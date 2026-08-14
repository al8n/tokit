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
     when no leg satisfies one; fails, naming the suite and its gate, when no leg compiles a
     suite BODY — then checks each declared leg's `unique_predicates` and `unique_suites`
     claims against what the tree actually says.
  5. Classifies every NEGATIVE combinator-family occurrence in `tokora/src/` and fails on any
     that is a real code gate rather than a doc-only `cfg_attr(..., doc = ...)`. Not a third
     coverage question: it is the precondition the subset dominance in (4) is sound under, and
     it prints the count it counted. See BOUNDS and `negative_family_gates`.

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
helper leaking into a suite would show up.

Nor is it "is there a feature every other covering leg has on that I have off". That is a
question about ONE feature, and a leg protects a CONFIGURATION — everything it has off, at
once. The two agree on today's tree and come apart on a change `tokora/Cargo.toml` already
discusses: split the umbrella into per-family `std,logos,<family> --tests` legs and every
family is on in some legs and off in others, so no single feature is common to all the
alternatives, while none of them compiles the eight with ALL thirteen off. The derived quantity
is therefore SUBSET DOMINANCE — another covering leg whose enabled features are a subset of
this one's has off everything this one has off — and `load_bearing_suites` carries the argument
for `<=` over `==`. Nothing names `combinators`; it falls out.

BOUNDS, stated so a green is not read as more than it is:

  * The PREDICATE enumeration is `tokora/src/` only, matching the issue's own enumeration.
    Files under `tokora/tests/` are read for their crate-level `#![cfg(...)]` gate and nothing
    else — the `cfg`s inside a suite body are not enumerated, so "this leg compiles the suite"
    means the body is type-checked in that configuration, not that every branch inside it is.
  * Suite coverage is measured over the same legs as predicate coverage, `--all-features`
    excluded for the same reason — and a suite no declared leg compiles is a FAILURE. It was
    reported on the green path until 2026-08-11, over three `rowan`-gated suites argued to be
    `--all-features`-only by design; two `cargo check` legs covered all three, so the design was
    a leg nobody had priced. A gate that knows about a gap in the thing it measures and stays
    green is the defect it exists to detect, arrived at from the reporting side.
  * SUBSET DOMINANCE IS SOUND FOR THE MONOTONE FRAGMENT, AND THE BOUND IS CHECKED. A predicate
    whose feature leaves all occur positively is false under a subset of the features whenever
    it is false here, so a dominating leg witnesses every absence the dominated one does. An
    item gated `cfg(not(feature = "x"))` inverts that: a leg with `x` ON is the one that sees it
    absent, and a leg with fewer features may not. `tokora/src/` does carry negative feature
    gates — on `std`, `alloc`, `smallvec_1`, the logos versions and `unstable-raw` — and the
    ones that would bite are the ones on the combinator-family axis, which is what the suite
    scan protects. `negative_family_gates` now enumerates those and fails on any that is a real
    code gate rather than a doc-only `cfg_attr(..., doc = ...)`, and the live count is printed
    on the green path BY THAT SAME PASS. It was a sentence with `32` copied into it from
    `tokora/Cargo.toml` until 2026-08-11, and the tree said 33. The sound-for-anything
    alternative is `==`, which is degenerate; this bound is the price of the answer meaning
    something, not an oversight.
  * THE PRECONDITION IS CHECKED OVER `tokora/src/` ONLY, matching the claim it replaces and the
    predicate enumeration above. A real negative family gate inside a suite BODY would break
    dominance the same way and is not seen; `tokora/tests/` carried none of either kind when
    this was measured on 2026-08-11, so widening the scan would change no outcome today — what
    it would change is the false-positive surface, since `tests/ui/` is a trybuild fixture tree
    of deliberately ill-formed files that is never compiled as a target.
  * DELETING A LEG'S ENTRY DELETES ITS CLAIM. `unique_predicates` survives deletion because the
    predicate stays in `src/` and goes uncovered; `unique_suites` does not always, because the
    suite is still compiled — just in a larger configuration — and nothing in the tree records
    that the smaller one was wanted. Measured on 2026-08-11 by deleting each declared leg in a
    copy and running this script: three of the five reddened, two did not. `--print-legs` is a
    declaration, and for those two the comment beside the leg is the only record of what its
    absence buys. Deleting an entry and re-running is how to find out which kind a leg is.
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
# no other leg compiling that suite has a SUBSET of this leg's features, which is what it would
# take to have off everything this leg has off. It is checked both ways too, and it is the
# reason a leg is never recommended for deletion on the strength of its predicates alone. See
# `load_bearing_suites` for why the criterion is `<=` and not `==`, and WHY THE SUITES ARE
# SCANNED in the module docstring for what it is protecting.
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
        # FALSE SINCE THE `rowan,logos,combinators` LEG BELOW EXISTED, AND THAT IS NOT A DEMOTION
        # OF THIS LEG. `rowan = ["dep:rowan", "std"]`, so that leg resolves to a strict SUPERSET
        # of this one and satisfies every predicate this one does; sole coverage is a claim about
        # the leg list, and the leg list grew. What holds this leg is `unique_suites`: it is the
        # smallest configuration compiling the 82 suites gated on the umbrella, the only one that
        # compiles them with `rowan` off. Delete the rowan legs and the gate will say to set this
        # back to True, in those words — the check runs in both directions.
        "unique_predicates": False,
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
        "why": "the smallest leg compiling the 82 umbrella-gated suites, and with `rowan` off",
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
        # crate-level gate `all(std, logos_0_16)` with no family and no umbrella — `boost`,
        # `error_extra`, `forwarding_discrimination`, `input_ref`, `lexer_error_paths`,
        # `recovery_terminal_stop`, `session_points`, `sync`. No `--each-feature` leg supplies
        # `std` AND logos at once, and every other configuration that does has `combinators` too
        # — every `--all-features` job (`test`, `test (release)`, `msrv`, `sanitizer`,
        # `coverage`), `test (valve-off)` and the Miri matrices (which name `--features logos`
        # WITHOUT `--no-default-features`, so `default` brings the umbrella), and the
        # `std,logos,combinators` and `rowan,logos,combinators` legs. So THIS is the only
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
        # any leg that still holds a suite. The first derivation asked whether one feature was on
        # in every alternative, which this leg satisfies only because `combinators` happens to be
        # that feature today; split the umbrella into per-family legs and it would have gone back
        # to recommending deletion. It is subset dominance now. This paragraph is the history,
        # not the mechanism.
        "why": "all(test, trace, logos_0_16, std)",
    },
    {
        "features": "rowan,combinators",
        "tests": True,
        "unique_predicates": False,
        "unique_suites": True,
        # DECLARED FOR SUITES, NOT FOR A PREDICATE. `tokora/src/` has no multi-feature predicate
        # naming `rowan` at all; what this leg is for is `tokora/tests/`, where
        # `cst_resource_trips` and `parser_node` carry the crate-level gate
        # `all(rowan, std, combinators)`. Until 2026-08-11 no declared leg compiled either, so
        # `--all-features` was their only build anywhere — the configuration this whole script
        # refuses to count as coverage — and the gate reported that in a green log line instead
        # of failing on it.
        #
        # `rowan = ["dep:rowan", "std"]`, so naming `rowan` reaches `std` and the feature string
        # does not have to. This is the LOGOS-OFF half of the pair: the only configuration that
        # type-checks those two bodies without a lexer, which is what would catch one of them
        # growing a dependency on the `logos` surface it does not gate on. Measured on
        # 2026-08-11 by planting an unconditional `compile_error!` in all three `rowan` suites:
        # this leg fired the two above and not `pratt_recovery`, which wants a logos version.
        "why": "cst_resource_trips + parser_node, the only build of them without a lexer",
    },
    {
        "features": "rowan,logos,combinators",
        "tests": True,
        "unique_predicates": False,
        "unique_suites": True,
        # The other half: `pratt_recovery` gates on `all(std, rowan, combinators, logos_0_16)`,
        # so it needs a lexer too and the leg above cannot reach it. Any leg that can is
        # a superset of `std,logos,combinators`, which is why that leg's `unique_predicates` is
        # now False — see the note there. It is not a reason to fold the two into one: this leg
        # has a lexer, so it cannot make the logos-off claim the leg above exists for.
        "why": "pratt_recovery, all(std, rowan, combinators, logos_0_16)",
    },
    {
        "features": "std,logos,combinators,tinyvec_1",
        "tests": True,
        "unique_predicates": False,
        "unique_suites": True,
        # DECLARED FOR ONE SUITE. `container_contract` gates on
        # `all(std, combinators, tinyvec_1, logos_0_16)`, and `tinyvec_1` is named by no other
        # crate-level gate and by no multi-feature predicate in `tokora/src/` — so until this leg
        # existed, `--all-features` was that body's only build anywhere, which is the
        # configuration this script refuses to count as coverage.
        #
        # The suite is what makes `tinyvec_1` worth a leg rather than a footnote: it is the file
        # that drives `SliceVec` through the refusal channel, and `SliceVec` is the adapter that
        # used to call the panicking upstream `push` and return `Ok(())` over it. A build that
        # never type-checks that body is a build in which the adapter's contract is unasserted.
        #
        # `tinyvec_1 = ["dep:tinyvec_1"]` pulls no other feature, so `std` and the umbrella have
        # to be named — and `logos` rather than `logos_0_16`, for the reason the
        # `std,logos,combinators` leg states at length.
        "why": "container_contract, the only declared build of a tinyvec_1-gated suite",
    },
    {
        "features": "std,logos,combinators,stacker",
        "tests": True,
        # Both claims are FALSE and the leg is kept anyway, exactly as `fold,alloc` is. Neither
        # derived quantity can see what this leg protects, and saying so is the point.
        #
        # `unique_predicates`: no predicate in `tokora/src/` names `stacker` beside another
        # feature. The two gates the feature owns are single-feature — `cfg(feature = "pratt")`
        # on `mod native_stack`, `cfg(feature = "stacker")` on the two `maybe_grow` arms inside
        # it — so the enumeration in (1) has nothing to attribute here. Writing an
        # `all(pratt, stacker)` predicate the code does not need, purely so this leg became
        # derivable, would be writing a `cfg` for the gate rather than for the program. The
        # manifest states that pair instead: `stacker = [..., "pratt"]`, so the two features
        # cannot come apart and no predicate has to say so.
        #
        # `unique_suites`: `std,logos,combinators --tests` is a strict SUBSET of this leg, so it
        # dominates every suite this one compiles.
        "unique_predicates": False,
        "unique_suites": False,
        # DECLARED FOR THE SUITES, NOT FOR THE LIBRARY. `stacker` composes with the `pratt`
        # family and with nothing else: `mod native_stack` is `pratt`-gated and its only two
        # callers are the two Pratt frame prologues — and since `stacker = [..., "pratt"]` the
        # `--each-feature` `stacker` leg now carries `pratt` with it and does compile that module
        # and both prologues. That closes the library half, and it is why this entry no longer
        # claims to be the only build of the pair.
        #
        # What `--each-feature stacker` still cannot reach is any suite that EXERCISES the pair:
        # 82 of the 106 integration suites gate on the umbrella and 79 of those want a logos
        # version too, so a `stacker`-plus-`pratt` leg without `std,logos,combinators` compiles
        # none of their bodies. Without this entry the only build that does is `--all-features`
        # — precisely the configuration the docstring refuses to count, for the reason it gives:
        # a third feature can supply whatever the pair is missing.
        #
        # What it buys, stated as what CI actually does with it and no wider: the
        # `feature combinations` job runs `cargo check -p tokora <leg>`, so this leg
        # TYPE-CHECKS the pratt wiring against the segmented prologue — with `--tests`, so the
        # suites' bodies too, and with `rowan` and `tinyvec_1` off. It does not execute them.
        # Executing the pair is `cargo test --all-features`'s job and stays there; what this leg
        # adds is the compile of the pair in a configuration that is not the everything-on one.
        #
        # `pratt_limit` carries three `#[cfg(feature = "stacker")]` cells — the segment-crossing
        # ones — that exist in no other declared leg's compile, and `recursion_tracker`'s literal
        # pins on `SEGMENTED_PRATT_DEPTH` are gated the same way.
        #
        # `stacker = ["dep:stacker", "std", "pratt"]`, so `std` and `pratt` arrive with it and are
        # named only for symmetry with the siblings; `logos` and the umbrella are named for the
        # reasons the `std,logos,combinators` leg states at length.
        "why": "the only leg compiling the stacker-gated suites against the pratt family",
    },
]

CRATE = "tokora"
SRC = pathlib.Path(CRATE) / "src"
TESTS = pathlib.Path(CRATE) / "tests"

# The umbrella the combinator-family axis is derived FROM, so that adding a family to the
# feature extends the guarded axis without anyone remembering to. See `negative_family_gates`.
UMBRELLA = "combinators"

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
    """Split on top-level commas, blind to anything inside a string literal.

    The mask matters to the second caller, not the first. `predicate_of` only ever reads part
    zero, which is a predicate and holds no comma outside `feature = "x"` — but `doc_only` asks
    what the parts AFTER the predicate are, and this crate really does carry
    `#[cfg_attr(not(feature = "filter"), doc = "…, prefer using `filter_map`…")]`. Counting that
    comma splits the doc text into a part that is not an attribute, and the classification the
    whole monotone bound turns on would answer "not doc-only" on a doc-only gate.
    """
    code, mask = lex(s)
    parts, depth, start = [], 0, 0
    for idx, ch in enumerate(code):
        if mask[idx]:
            continue
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif ch == "," and depth == 0:
            parts.append(code[start:idx])
            start = idx + 1
    parts.append(code[start:])
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


DOC_ATTR = re.compile(r"doc\s*[=(]")


def doc_only(body: str) -> bool:
    """True when an attribute gates NOTHING BUT documentation.

    `#[cfg(...)]` is never doc-only: it decides whether the item exists at all. `#[cfg_attr(P,
    A, ...)]` is, exactly when every `A` is a `doc` attribute — `doc = "…"` or `doc(...)`.
    Anything else applied under a `cfg_attr` is a real compilation difference: `derive(...)` is
    the obvious one and `cfg_attr(P, cfg(Q))` the sly one, and it is the same reason
    `predicate_of` returns a `cfg_attr`'s predicate at all rather than skipping the form.

    A `cfg_attr` applying no inner attribute is malformed Rust and answers False. The
    conservative direction for this question is the one that reds: see `negative_family_gates`
    for what a wrong `True` here would let through.
    """
    if not (body.startswith("cfg_attr(") and body.endswith(")")):
        return False
    inner = split_top(body[9:-1])[1:]
    return bool(inner) and all(DOC_ATTR.match(a) for a in inner)


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


def negative_family_gates(root: pathlib.Path, axis: set[str]):
    """`([(site, families)], [(site, predicate, families)], negatives)` — the antitone gates.

    THE PRECONDITION `load_bearing_suites` RESTS ON, MADE EXECUTABLE. Subset dominance drops a
    leg for a suite when a smaller leg compiles it, on the argument that a smaller configuration
    has off everything this one has off and therefore witnesses the same absences. That argument
    is monotone-only. An item gated `cfg(not(feature = "map"))` is COMPILED when `map` is off, so
    the smaller leg HAS the item and this one does not — the dominator fails to witness exactly
    the absence this leg was keeping, and the leg is called redundant when it is the only thing
    standing between that configuration and nobody building it.

    Two products, because the whole bound turns on telling them apart, and one count.

      * A REAL CODE GATE — `#[cfg(not(...))]`, or a `cfg_attr` applying anything but `doc` —
        changes what is compiled. It is returned as a failure.
      * A DOC-ONLY `#[cfg_attr(not(feature = "map"), doc = "…")]` degrades an intra-doc link to
        plain prose and compiles the same items either way, so it is inert for this question.
        Every occurrence in the tree on 2026-08-11 was that shape, so a check that could not tell
        the two apart would red on the unmodified tree and be deleted within the week.

    THE AXIS IS DERIVED, NOT LISTED: `combinators` and everything it pulls, so a fourteenth
    family is guarded the day it joins the umbrella. It is the combinator-family axis and not
    every feature because that is the axis the suite scan protects — `tokora/src/` carries real
    negative gates on `std` and `alloc` by construction, being a no-std crate, and a check that
    reddened on those would be asking the crate to stop supporting no-std. What that leaves
    unguarded is stated in BOUNDS.

    `negatives` counts negative feature leaves over ALL features, not just the axis, and exists
    as a positive control: `tokora/src/` cannot stop carrying negative `std` and `alloc` gates
    without ceasing to be no-std, so zero is the polarity walk having stopped matching rather
    than the tree having changed. It is stated as a property and not as a figure on purpose: the
    only number this function produces is the one it returns, and the caller prints that.

    The AXIS count is deliberately NOT controlled the same way. It can legitimately reach zero by
    someone rewriting doc gates, so a control forbidding that would pin a number — which is the
    defect this check exists to stop this file from repeating for a fourth time.
    """
    doc_gated: list[tuple[str, list[str]]] = []
    code_gated: list[tuple[str, str, list[str]]] = []
    negatives = 0
    for path in sorted(root.rglob("*.rs")):
        text, mask = lex(path.read_text())
        for off, body, _end in attributes(text, mask):
            pred = predicate_of(body)
            if pred is None:
                continue
            norm = re.sub(r"\s+", " ", pred).strip()
            try:
                node = parse(norm)
            except ValueError:
                # Not this check's business. A predicate that does not parse is already reported
                # by the coverage pass when it names two or more features, and reddening here
                # too would print two messages for one defect — the less useful one second.
                continue
            found = negative_features(node, [])
            negatives += len(found)
            families = sorted({f for f in found if f in axis})
            if not families:
                continue
            line = text.count("\n", 0, off) + 1
            site = f"{path}:{line}"
            if doc_only(body):
                doc_gated.append((site, families))
            else:
                code_gated.append((site, norm, families))
    return doc_gated, code_gated, negatives


def load_bearing_suites(suite_hits, features_of):
    """`{leg flags: {suite name: what the alternatives add}}` — the suites a leg alone protects.

    Deleting a leg is safe for a suite when another leg REPRODUCES this one's configuration of
    it, and everything turns on what "reproduces" is allowed to mean. Three candidates; the
    middle one is the only one that is both sound and says anything.

    "SOLE COMPILE" answers itself. Every leg builds a suite under a feature set no other leg
    has, so every leg is sole something and nothing is ever deletable.

    "IS THERE A FEATURE EVERY OTHER COVERING LEG HAS ON THAT I HAVE OFF" — the intersection of
    the others, minus mine — asks about ONE feature. What a leg protects is a CONFIGURATION:
    everything it has off, at once. Split the alternatives finely enough and no single feature
    is common to all of them while the configuration is reproduced by none. That is not
    hypothetical here. Add the per-family `std,logos,<family> --tests` legs that
    `tokora/Cargo.toml` already discusses under WHAT RE-GATING THE 82 WOULD NOT BUY, and every
    combinator family becomes absent from some other leg, so the intersection for
    `std,logos,trace` empties — while no remaining leg compiles those eight suites with all
    thirteen families off, which is the only reason that leg is declared. Case H of the
    self-test is that tree, and it is red under the intersection.

    SUBSET DOMINANCE is what is used: a leg is dropped from the result for a suite when some
    other leg compiling it has enabled features that are a SUBSET of this leg's. That leg has
    off everything this one has off, so every absence this one witnesses it witnesses too.

    `<=` AND NOT `==`, and the fork is worth the paragraph because the next person meets it:

      * `==` is sound for ANY predicate, negative ones included, and it is degenerate. Every
        declared leg resolves to a distinct feature set — one that did not would be a duplicate,
        which `main` now reds on — so `==` finds no dominator anywhere, marks every `--tests`
        leg load-bearing for every suite it compiles, and lands back on "sole compile" one layer
        down. Measured on the 2026-08-11 tree: 43 of 43 test legs held something under `==`,
        against 8 under `<=`.
      * `<=` is sound for the monotone fragment. A predicate whose feature leaves all occur
        positively is false under a subset of the features whenever it is false here, so the
        absence carries over. Where it does not carry over is stated in BOUNDS.
      * The two errors are not symmetric, because the answer is used to REFUSE deletion. Marking
        a leg load-bearing when it is not keeps a leg somebody could have pruned. Failing to mark
        one prints `delete the leg` over a configuration nothing else builds, with CI green —
        the defect the whole suite scan exists for. `<=` marks a superset of what the
        intersection marked (a leg with a dominator always had an empty remainder), so it can
        only move legs into the protected set, never out.

    The value recorded is what the alternatives ADD: the union, over the other legs compiling
    the suite, of the features they have and this one does not. Every one of them is off in this
    leg and every other leg compiling the suite turns at least one of them on, which is the
    sentence the output prints. It is empty exactly when no other leg compiles the suite at all
    — nothing needs saying about which features are off when the alternative is not compiling
    it — and those are reported separately.
    """
    out: dict[str, dict[str, frozenset]] = {}
    for path, hits in suite_hits.items():
        for flags in hits:
            mine = features_of[flags]
            others = [features_of[o] for o in hits if o != flags]
            if any(other <= mine for other in others):
                continue
            out.setdefault(flags, {})[path.name] = frozenset().union(
                *(other - mine for other in others)
            )
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


def negative_features(node, acc: list[str], negated: bool = False) -> list[str]:
    """The feature leaves under an ODD number of `not`s — the antitone occurrences.

    Parity, not "is there a `not` anywhere above me": `not(not(feature = "x"))` is monotone
    again, and `all` / `any` do not flip anything. A LIST and not a set, because two negative
    occurrences of one feature in one predicate are two occurrences and the figure this feeds is
    a count of occurrences — the same figure a comment used to state, wrongly.
    """
    kind = node[0]
    if kind in ("all", "any"):
        for child in node[1]:
            negative_features(child, acc, negated)
    elif kind == "not":
        negative_features(node[1], acc, not negated)
    elif negated and node[1] == "feature" and node[2] is not None:
        acc.append(node[2])
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
    family_axis = closure([UMBRELLA], fmap) if UMBRELLA in fmap else set()
    doc_gated, code_gated, negatives = negative_family_gates(SRC, family_axis)

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
    if UMBRELLA not in fmap:
        problems.append(f"{CRATE} declares no {UMBRELLA!r} feature, so the combinator-family "
                        f"axis subset dominance is bounded on cannot be derived and the "
                        f"precondition check below would pass by looking at nothing")
    if negatives == 0:
        problems.append(f"found 0 negative `not(feature = ...)` occurrences under {SRC}; this "
                        f"crate carries them on `std` and `alloc` alone by being no-std, so the "
                        f"polarity walk enforcing the subset-dominance precondition stopped "
                        f"matching")
    # Two legs that resolve to the same feature set are not two legs. Everything below is a
    # question about what one leg reaches that another does not, and a pair like that answers
    # `nothing` in both directions at once — under `<=` each one dominates the other, so both
    # look deletable and deleting both is wrong. Cheaper to red than to make the dominance
    # relation carry a tie-break nobody would find.
    resolved: dict[tuple, list[str]] = {}
    for flags, feats, tests in all_legs:
        resolved.setdefault((feats, tests), []).append(flags)
    for group in sorted(sorted(g) for g in resolved.values() if len(g) > 1):
        problems.append(
            f"legs {' and '.join('`' + f + '`' for f in group)} resolve to the same feature set, "
            f"so neither reaches anything the other does not. Delete one, or name the feature "
            f"that was meant to differ"
        )
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

    # ── Integration suites no declared leg compiles ──────────────────────────────────────────
    # This was computed, printed on the green path and not enforced until 2026-08-11, with a
    # BOUNDS paragraph arguing the three then-current ones were `--all-features`-only by design.
    # A green log line is not an enforcement mechanism: the gate knew about a gap in the thing
    # it measures and stayed green, which is the same defect as a leg list that has gone stale,
    # arrived at from the reporting side. The three were `rowan`-gated and cost two `cargo
    # check` legs to cover, so the argument for waiving them was never the argument it claimed
    # to be. It is a failure now, and the two legs are declared above.
    orphans = [(path.name, suites[path]) for path, hits in suite_hits.items() if not hits]
    if orphans:
        print(f"feature-cfg-coverage: {len(orphans)} integration suite"
              f"{'' if len(orphans) == 1 else 's'} no declared leg compiles", file=sys.stderr)
        print("", file=sys.stderr)
        for name, gate in orphans:
            print(f"  {TESTS / name}", file=sys.stderr)
            print(f"    #![cfg({gate})]", file=sys.stderr)
        print("", file=sys.stderr)
        print("`--all-features` is the only build that type-checks "
              f"{'these bodies' if len(orphans) > 1 else 'this body'}, and `--all-features` "
              "satisfies every predicate by construction — a suite reachable from nowhere else "
              "is a suite whose only compile is in the configuration this gate exists to "
              "distrust. It is also invisible to `cargo hack --each-feature`, which never runs "
              "a proper subset of size two or more.", file=sys.stderr)
        print("", file=sys.stderr)
        print("Fix: add a leg to EXTRA_LEGS in ci/feature_cfg_coverage.py naming the smallest "
              "feature set that satisfies the crate-level gate, with `\"tests\": True` — "
              "without it the leg builds no integration target and compiles no suite at all. "
              "CI runs whatever that list prints.", file=sys.stderr)
        return 1

    # ── The precondition subset dominance rests on ───────────────────────────────────────────
    # It sits here, immediately above its only consumer, because that is what it is: everything
    # `load_bearing_suites` concludes below is conditional on it, and a reader who meets the
    # dominance argument should have met its precondition one screen earlier.
    #
    # UNTIL 2026-08-11 THIS WAS A SENTENCE, and both halves of it failed the same way. The claim
    # — no real `not(feature = "<family>")` code gate in `tokora/src/` — was checked by nobody,
    # so a future one would have made the criterion unsound with CI green, which is this script's
    # own defect class reached through its own soundness argument. And the count beside it was
    # written as 32, taken from a comment in `tokora/Cargo.toml`, and was 33 by the time a
    # reviewer recounted it — in a change whose entire thesis is that a written-down count drifts
    # and must be derived, and which had just removed exactly such a count from the leg list.
    # Third instance in this file's history, and the first two are described where they happened.
    #
    # So the count printed on the green path comes out of THIS pass, the one that enforces the
    # bound. A check that enforces one thing and prints another is the same defect one layer on.
    if code_gated:
        n = len(code_gated)
        print(f"feature-cfg-coverage: {n} real combinator-family `not(feature = ...)` code "
              f"gate{'' if n == 1 else 's'} under {SRC}", file=sys.stderr)
        print("", file=sys.stderr)
        for site, pred, families in code_gated:
            print(f"  {site}", file=sys.stderr)
            print(f"    cfg({pred})  — negative on {', '.join(families)}", file=sys.stderr)
        print("", file=sys.stderr)
        print("Subset dominance is sound only for the monotone fragment, and this breaks it. "
              "`load_bearing_suites` drops a leg for a suite when another leg compiling that "
              "suite has a SUBSET of its features, on the argument that the smaller "
              "configuration has off everything this one has off. A negatively gated item is "
              "COMPILED in the smaller configuration and absent from this one, so the dominator "
              "does not witness this leg's absence of it — and the leg is called redundant when "
              "it is the only configuration keeping that code out. Deletion advice follows from "
              "that, which is the failure this whole suite scan exists to prevent.",
              file=sys.stderr)
        print("", file=sys.stderr)
        print("The doc-only occurrences are not this: `cfg_attr(not(feature = ...), doc = ...)` "
              "changes documentation text and compiles the same items either way, and every "
              "occurrence in the tree was that shape when this check was added.", file=sys.stderr)
        print("", file=sys.stderr)
        print("Fix: make the gate doc-only, or gate the item positively on what it needs. If a "
              "real negative family gate has to stay, the criterion itself has to change — "
              "`load_bearing_suites` would need `==`, which is sound for any predicate and was "
              "measured DEGENERATE (43 of 43 test legs load-bearing, against 8 under `<=`), so "
              "it does not weaken the answer, it stops there being one. Read the `<=` versus "
              "`==` fork there before reaching for it.", file=sys.stderr)
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

    def additions(witness: dict[str, frozenset]) -> str:
        """`X, Y` — what the alternatives add, reduced to the features that imply the rest.

        Read jointly, never one at a time: every one of these is OFF in this leg, and every
        other leg compiling those suites turns at least ONE of them on. Some of them will be off
        in some other leg — that is the whole point of the fork in `load_bearing_suites`, and
        a per-feature reading of this line would be the intersection question again.

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
                f"still the only leg that compiles {held} without {additions(witness)} "
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
                f"compiles is also compiled by a leg whose features are a subset of this one's "
                f"— which therefore has off everything this one has off. Fix the claim — and "
                f"read what the leg's comment says it protects before concluding it protects "
                f"nothing."
            )
        if not leg["unique_suites"] and witness:
            drift.append(
                f"leg `{flags}` declares unique_suites=False, but it is now the only leg "
                f"compiling {held} without {additions(witness)} ({listing(witness.keys())}). Set "
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
    gated = sum(1 for g in suites.values() if g)
    print(f"feature-cfg-coverage OK: {len(predicates)} multi-feature cfg predicates over "
          f"{sum(len(v) for v in predicates.values())} sites in {files} files, "
          f"{len(all_legs)} covering legs ({len(EXTRA_LEGS)} declared, --all-features excluded)")
    print(f"  and {len(suites)} integration-test targets under {TESTS}, {gated} of them "
          f"crate-gated")
    # THE COUNT COMES OUT OF THE CHECK, not from beside it. This is the figure `code_gated` was
    # empty of; printing any other one would reintroduce the drift the check was added for.
    occurrences = sum(len(f) for _site, f in doc_gated)
    print(f"  and {occurrences} negative combinator-family cfg occurrence"
          f"{'' if occurrences == 1 else 's'} over {len(doc_gated)} attribute"
          f"{'' if len(doc_gated) == 1 else 's'}, every one doc-only — the monotone bound "
          f"subset dominance rests on, enumerated by the check that enforces it")
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
              f"{'' if len(witness) == 1 else 's'} no other leg compiles with everything this "
              f"one has off")
        if alone:
            print(f"          {len(alone):3d} compiled by no other leg at all")
        if shared:
            print(f"          {len(shared):3d} compiled elsewhere, only by legs that add "
                  f"{additions(shared)}")
        print(f"      why: {leg['why']}")
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
