#!/usr/bin/env python3
"""The shard plan for the Miri jobs — the single source of truth for how the suite is split.

Miri set the pull request's wall clock on its own. One completed run measured the worst cell,
`miri-tb-x86_64-unknown-linux-gnu`, at 116.9 minutes against 9 minutes for the next-longest job
in the workflow; the six Miri cells together were 437 of the run's ~490 minutes. Nothing else
was close, so nothing else was worth splitting.

The split is by integration test target, and the target list is ENUMERATED AT RUN TIME from
`cargo metadata`. A hardcoded list would mean a newly added `tokora/tests/*.rs` is silently
never interpreted under Miri, and nothing would ever say so — the shards would stay green while
covering less every month. Live enumeration keeps coverage exhaustive regardless of which
algorithm then assigns shards: every enumerated target ends up in exactly one shard, so a new
file joins some shard the moment it exists, without anyone remembering to add it.

WHICH shard is decided by greedy LPT (Longest Processing Time first) over `TARGET_WEIGHTS`, not
by `i % SHARD_TOTAL`. Splitting by target COUNT assumed every target costs the same to
interpret; measured (#178), the 102 targets range from 0.0s to 261.2s per run — an outlier
~87x the median — and that range does not track file size (the largest test file by line count
is one of the cheaper targets to interpret; a mid-sized one is the single most expensive). A
count-balanced split can hand one shard several of the expensive tail and another shard none of
it, which is exactly what happened: Linux `miri-tb`'s four shards measured 77.5 / 64.1 / 23.3 /
12.8 minutes, shard 0 doing roughly 6x shard 3's work. LPT sorts targets by descending weight
and assigns each to the shard currently carrying the least total weight — the standard greedy
algorithm for this exact problem, with a known worst-case bound (at most 4/3 of optimal,
converging to it as `SHARD_TOTAL` grows) rather than an ad hoc heuristic. See `TARGET_WEIGHTS`
below for where the weight per target comes from and how a target with no entry is handled.

Three units cannot be split by test target and are each PINNED to exactly one shard so they are
paid once instead of `SHARD_TOTAL` times:

    lib unit tests, `--features logos`                 → shard 0
    doctests                                           → shard 0
    lib unit tests, `--features logos,unstable-raw`    → shard 1 (`1 % SHARD_TOTAL`)

The two lib runs are deliberately on DIFFERENT shards. They are the largest indivisible units in
the job — 31.6 and 32.1 minutes on the worst cell, 55% of it between them — so putting both on
one shard would leave that shard at ~64 minutes and cap the whole exercise there. Split across
two shards they overlap, and the cell's critical path becomes one lib run plus one integration
slice. `1 % SHARD_TOTAL` rather than a bare `1` keeps the pinning total: at `SHARD_TOTAL = 1`
the raw lib run lands back on shard 0 rather than on a shard that does not exist.

──────────────────────────────────────────────────────────────────────────────────────────────
THE GUARD, and why it is written the way it is
──────────────────────────────────────────────────────────────────────────────────────────────

Read `ci/changelog_structure.sh`'s header first. It records two production failures in this
repository that were one defect wearing two hats: a check whose answer depended on where
something sat, so it examined nothing and reported success. Sharding is a fresh place for that
same defect, and a worse one — a shard that enumerates no targets does not merely check less,
it checks nothing, and a green tick from it is indistinguishable from a green tick from a shard
that did the work.

So every assertion below is a HARD FAILURE, run in every mode, on every shard, before a single
test is interpreted. "Enumerated nothing" exits non-zero. It is never a quiet skip, an empty
flag list, or a shard that finishes early and passes.

  (1) `cargo metadata` ran, exited zero, and parsed. A failed enumeration is not an empty
      enumeration.

  (2) Exactly one workspace package is named `tokora`. Zero means the filter is stale — a
      rename would otherwise produce an empty target list that looks exactly like a package
      with no tests.

  (3) The enumerated count is greater than zero. The positive control: without it, "the query
      matched nothing" and "the crate has no integration tests" are the same green.

  (4) The enumerated set equals the set cargo's own autodiscovery implies from `tokora/tests/`
      — `*.rs` files plus `*/main.rs` directories. This is the assertion that (3) cannot make.
      A count of 101 is not evidence of anything if 140 files exist; only an INDEPENDENT second
      source of truth can catch the metadata query silently narrowing (an `autotests = false`,
      a `[[test]]` block with a custom path, a future workspace layout change). When these two
      disagree the correct outcome is red, not a quiet majority vote, and the message says
      which side is missing what.

  (5) Names agreeing is not files agreeing. A `[[test]]` block can keep an existing target's
      NAME while pointing its `path` at a different file; the set comparison in (4) sees two
      equal sets of names and cannot tell the difference. So every metadata target's `src_path`
      is compared against the file `tokora/tests/` implies for that same name — or, for a
      declared `PATH_EXCEPTIONS` entry, against the approved override — and a HARD FAILURE
      names the target and both paths on any mismatch, any name claimed by two metadata targets
      at once, or any unrecognised custom path. Without this, (4) proves the right names exist
      and nothing about whether they still mean what they say.

  (6) `1 <= SHARD_TOTAL <= len(targets)`, so no shard can be empty by arithmetic.

  (7) The partition is verified against the enumerated list on every invocation: every shard
      non-empty, the shards pairwise disjoint, and their union equal to the full list. Each
      shard re-proves the whole partition rather than trusting its own slice, so a broken split
      reddens all of them at once instead of silently dropping the one nobody looks at.

  (8) Every target name matches `[A-Za-z0-9_-]+`. The emitted flags are word-split unquoted by
      the calling shell — that is intentional, it is how a variable becomes many arguments — and
      this assertion is what keeps that split from ever producing a shell metacharacter or an
      empty word. It does NOT, on its own, stop a target named e.g. `--help` from being read as
      a flag instead of a value: two bare words, `--test` then `--help`, let cargo's own arg
      parser treat `--help` as the global help flag and exit 0 having run nothing — a green
      shard that did no work. So the flag and the name are joined into one word, `--test=name`,
      which is always a single value no matter what it starts with; this regex is defense in
      depth underneath that join, not the thing that makes it safe.

Point (7) has a second reason to be strict that is specific to `cargo test`: an EMPTY selector
list is not "run nothing", it is "run everything". `cargo miri test --target T --features logos`
with no `--test` flag interprets all 102 targets. A shard whose slice came back empty would
therefore not fail fast and would not run fast — it would quietly do the entire suite, four
times over, and pass. Refusing an empty slice is the only thing standing between a plan bug and
a four-fold increase in the wall clock this file exists to reduce.

`SHARD_TOTAL` lives here and nowhere else. The workflow does not spell out `[0, 1, 2, 3]`; the
`miri-shard-plan` job asks this file for the shard list and the Miri matrices are built from
that output. A shard list and a shard count that are written down twice can disagree, and the
disagreement that matters — a shard dropped from the list while the count stays — removes a
quarter of the suite from CI without reddening anything.

Sorting is Python's, not the shell's: `sort(1)` is locale-dependent and the runners are not all
the same OS, so a shell sort could hand two runners different partitions for the same list.

`selftest` (below) exercises every guard above except (1). That one guard is the live
`cargo metadata` subprocess call succeeding at all — by design the selftest never shells out to
cargo or reads the real workspace, so there is nothing for it to fail in that specific way. The
other seven are all driven with mocked metadata, a throwaway tree, or a hand-built shard list,
because `verify_targets` and `verify_partition` take that input as plain arguments rather than
reading it themselves.

Modes:

    plan                    validate, print the partition, and emit `shards=[…]` to
                            `$GITHUB_OUTPUT` for the Miri matrices to consume.
    flags <shard> logos     `[--lib ]--test=a --test=b …` for the `logos` pass.
    flags <shard> raw       `[--lib ]--test=a --test=b …` for the `logos,unstable-raw` pass.
    flags <shard> doc       `--doc` if this shard owns the doctests, else the literal `SKIP`.
    selftest                exercise every testable guard — (2) through (8), skipping only (1)
                            — against mocked `cargo metadata` JSON, hand-built shard lists, and
                            a throwaway `tests/` tree; no real cargo invocation, no edit to the
                            real manifest. Exits non-zero if any case fails.

`doc` reports `SKIP` rather than printing nothing because an empty string and a crashed helper
look identical to a shell, and one of them must not be read as "no doctests to run".
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

# The number of shards. Changing this number is the whole knob: the matrices, the pinning and
# the partition all follow from it. See the header for why it is not also written in the
# workflow.
#
# 4 is the knee. The integration work is ~50 minutes on the worst cell and divides cleanly, but
# the indivisible lib runs are ~32 minutes each, so the cell cannot go below ~33 minutes however
# far this is pushed: 3 shards give ~50 minutes, 4 give ~46, 6 give ~42, 8 give ~40. Past 4 each
# extra shard buys a couple of minutes on Linux and costs two more macOS jobs, and macOS is the
# scarce resource — GitHub caps macOS at 5 concurrent jobs per account on the Free, Pro and Team
# plans, and every Miri cell but two runs there.
SHARD_TOTAL = 4

PACKAGE = "tokora"

# The pinned units. See the header for why the two lib runs are on different shards.
LIB_LOGOS_SHARD = 0
DOC_SHARD = 0
LIB_RAW_SHARD = 1 % SHARD_TOTAL

NAME_RE = re.compile(r"\A[A-Za-z0-9_-]+\Z")

# Guard (5)'s escape hatch: a target name intentionally served from a path other than the one
# `tokora/tests/` implies (a `[[test]]` block with a deliberate custom `path`). Empty today —
# there is no `[[test]]` block in `tokora/Cargo.toml` — and every entry here is a standing claim
# that a human, not this script, checked the target still covers what its name promises. Keys
# are target names; values are the approved source path, relative to the repository root.
PATH_EXCEPTIONS: dict[str, str] = {}

# Per-target LPT weight: seconds to interpret that target under Miri, averaged over the two
# feature passes (`logos` and `logos,unstable-raw`) `miri_tb.sh`/`miri_sb.sh` run per shard.
# Measured from GitHub Actions run 30904621015 (main @ b78e854, 2026-08-04), job
# `miri-tb-x86_64-unknown-linux-gnu-shard{0,1,2,3}`, by pairing each `Running tests/<name>.rs`
# line in the raw job log with the `finished in <N>s` line that follows it. Real interpreted
# cost, not a size proxy: file size was checked and rejected as a stand-in — the largest test
# file by line count (`parser_delim`) is one of the cheaper targets below, and a mid-sized file
# (`pratt_txn_retention`) is the single most expensive.
#
# A single-run snapshot, not a byte-identity requirement on this file: Miri timing drifts with
# the suite and with shared-runner noise, and nothing here asserts this table still matches a
# live run. It exists so `partition()` is weight-AWARE instead of uniform, which it was not at
# all before #178. Refresh by re-parsing a recent green Miri job log the same way when the skew
# below drifts far enough to matter again. A live target with no entry here — added since this
# was captured — falls back to `DEFAULT_WEIGHT` rather than being dropped or refused; see the
# `note()` in `partition()` for how that fallback is surfaced rather than silently absorbed.
TARGET_WEIGHTS: dict[str, float] = {
  "absence_terminal_stop": 7.0,
  "boost": 7.6,
  "bstr_gate": 0.0,
  "bundle_elaboration": 1.0,
  "bytes_gate": 0.0,
  "cmpl_pins": 1.0,
  "collection_resource_trip": 20.2,
  "collection_terminal_stop": 3.1,
  "combinators": 3.1,
  "container": 1.9,
  "delim_branded_lang": 0.8,
  "delim_unclosed": 4.9,
  "delim_wrong_opener_eoi": 4.4,
  "delimited": 2.6,
  "delimited_edges": 6.0,
  "delimited_errors": 10.8,
  "delimited_front_witness": 58.8,
  "delimited_route_parity": 2.0,
  "diagnostics": 0.0,
  "dialect_anchor": 0.5,
  "dispatch": 4.6,
  "dispatch_terminal_stop": 0.9,
  "downcast": 0.3,
  "emitter": 9.8,
  "end_state_parity": 9.9,
  "error": 19.5,
  "error_extra": 7.6,
  "escaped": 3.4,
  "expect": 3.1,
  "expect_with_cache": 4.3,
  "handler": 18.9,
  "handler_coverage": 22.1,
  "handler_extra": 12.6,
  "hipstr_gate": 0.0,
  "input_ref": 5.0,
  "keyword": 6.0,
  "lexer_error_paths": 2.3,
  "lit_token": 5.5,
  "misc": 5.4,
  "missing_separator": 1.2,
  "nonparser": 28.7,
  "parse_input": 2.3,
  "parser_atoms": 9.6,
  "parser_basic": 3.4,
  "parser_delim": 25.1,
  "parser_extra": 2.3,
  "parser_fold": 4.2,
  "parser_misc": 6.5,
  "parser_node": 0.0,
  "parser_padded": 5.7,
  "parser_pratt": 2.9,
  "parser_recover": 1.9,
  "parser_repeated": 5.0,
  "parser_repeated_delim": 4.1,
  "parser_repeated_while": 8.7,
  "parser_small": 48.0,
  "pratt_end": 1.0,
  "pratt_floor": 25.7,
  "pratt_limit": 162.1,
  "pratt_limit_unit_sink": 122.8,
  "pratt_prefix_progress": 60.0,
  "pratt_progress_guard": 166.8,
  "pratt_txn_retention": 261.2,
  "probe_close_no_rescan": 1.3,
  "punct": 2.6,
  "punct_branded_lang": 0.4,
  "recovery_terminal_stop": 1.9,
  "render_freeze": 1.0,
  "repeated_while_max_hook_order": 1.3,
  "sep_delim": 19.7,
  "sep_delim_extra": 6.5,
  "sep_delim_mutref": 6.3,
  "sep_parse": 14.1,
  "sep_parse_extra": 4.5,
  "sep_parse_mutref": 5.9,
  "sep_require": 18.0,
  "sep_while": 10.1,
  "sep_while_delim": 6.9,
  "sep_while_delim_extra": 6.1,
  "sep_while_delim_mutref": 6.5,
  "sep_while_delim_terminal_stop": 0.6,
  "sep_while_parse": 14.1,
  "sep_while_parse_extra": 6.0,
  "sep_while_parse_mutref": 6.2,
  "sep_while_terminal_stop": 0.6,
  "separated": 10.9,
  "separator_delivery": 2.0,
  "separator_position": 1.3,
  "session_points": 0.9,
  "shape_unclosed": 4.4,
  "span": 63.7,
  "state_machine": 10.9,
  "sync": 3.6,
  "token_error": 2.2,
  "tracker": 5.0,
  "tryshape_terminal_stop": 3.1,
  "typed_unclosed": 0.8,
  "types": 12.4,
  "unclosed_kind_dispatch": 1.0,
  "utils": 7.4,
  "verbose_conformance": 17.6,
  "wapi_b": 5.7,
}

# The median of TARGET_WEIGHTS' values at capture time, used for any live target the table does
# not name. Median rather than mean: the distribution is dominated by a handful of outliers
# (five targets over 60s against this 5.0s median; the mean is ~15s), so the mean would overstate
# a typical new target while still understating what the tail actually costs. An unweighted
# target is not a failure — `partition()` still produces a valid, exhaustive, disjoint split,
# see guards (6)-(7) — only one LPT cannot yet balance correctly against real cost.
DEFAULT_WEIGHT = 5.0


def die(msg: str) -> None:
    """Every failure in this file goes through here: one prefix, and always exit 1."""
    print(f"miri_shard: FAIL: {msg}", file=sys.stderr)
    sys.exit(1)


def note(msg: str) -> None:
    print(f"miri_shard: {msg}", file=sys.stderr)


def repo_root() -> Path:
    """Derived from this file's location, never from the working directory.

    Same reason `ci/changelog_structure.sh` refuses position-dependent answers: a helper that
    resolves its inputs relative to wherever it happened to be invoked from is a helper that
    can be made to find nothing.
    """
    return Path(__file__).resolve().parent.parent


def autodiscovered_paths(root: Path) -> dict[str, Path]:
    """The integration test target name -> canonical source file cargo's autodiscovery implies.

    `tests/*.rs` is one target per file, named for the stem; `tests/<dir>/main.rs` is one
    target named for the directory. A directory without a `main.rs` — `tests/common/`, which is
    a shared module — is not a target. This mirrors cargo's documented rules and exists only to
    disagree with `cargo metadata` when something has gone wrong.

    Returns a name -> path map rather than just names, because a name by itself does not prove
    which file it covers — see guard (5) in `verify_targets`, which is the reason this keeps
    the path instead of throwing it away the way a bare set of names would.
    """
    tests_dir = root / PACKAGE / "tests"
    if not tests_dir.is_dir():
        die(f"`{tests_dir}` is not a directory; the integration suite has moved or vanished")
    paths: dict[str, Path] = {}
    for entry in sorted(tests_dir.iterdir()):
        if entry.is_file() and entry.suffix == ".rs":
            name, found = entry.stem, entry
        elif entry.is_dir() and (entry / "main.rs").is_file():
            name, found = entry.name, entry / "main.rs"
        else:
            continue
        if name in paths:
            die(
                f"`tokora/tests/` implies target `{name}` from two different sources: "
                f"{paths[name]} and {found}. cargo cannot give both the same target name, so "
                "this file cannot pick a canonical path for either — rename one."
            )
        paths[name] = found
    return paths


def autodiscovered_names(root: Path) -> set[str]:
    """The integration test target names cargo's autodiscovery implies from the source tree.

    A thin wrapper over `autodiscovered_paths` so guard (4)'s name-only comparison and guard
    (5)'s path comparison share one source of truth instead of two trees that can drift apart.
    """
    return set(autodiscovered_paths(root))


def enumerate_targets() -> list[str]:
    """Guard (1): runs `cargo metadata`, parses it, and hands off to `verify_targets` for
    guards (2)-(5) and (8). Returns the sorted integration test target names.
    """
    root = repo_root()
    manifest = root / "Cargo.toml"

    try:
        proc = subprocess.run(
            [
                "cargo",
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
                str(manifest),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as exc:  # cargo missing entirely
        die(f"could not run `cargo metadata`: {exc}")

    # (1) A failed enumeration is not an empty enumeration.
    if proc.returncode != 0:
        die(
            f"`cargo metadata` exited {proc.returncode} for {manifest}\n"
            f"--- stderr ---\n{proc.stderr.strip()}"
        )
    try:
        meta = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        die(f"`cargo metadata` did not emit parseable JSON: {exc}")

    return verify_targets(meta, root)


def verify_targets(meta: dict, root: Path) -> list[str]:
    """Guards (2)-(5) and (8), given already-parsed `cargo metadata` JSON and a repo root.
    Returns the sorted integration test target names.

    Split out from `enumerate_targets` so `mode_selftest` can drive these guards with mocked
    metadata and a throwaway `tests/` tree — never a real `cargo metadata` invocation, never a
    write to the real manifest.
    """
    # (2) A stale package filter must not read as a package with no tests.
    pkgs = [p for p in meta.get("packages", []) if p.get("name") == PACKAGE]
    if len(pkgs) != 1:
        found = sorted(p.get("name", "?") for p in meta.get("packages", []))
        die(
            f"expected exactly one workspace package named `{PACKAGE}`, found {len(pkgs)}; "
            f"workspace packages: {found}"
        )

    test_targets = [t for t in pkgs[0].get("targets", []) if t.get("kind") == ["test"]]
    names = sorted(t["name"] for t in test_targets)

    # (3) The positive control.
    if not names:
        die(
            f"`cargo metadata` reported ZERO integration test targets for `{PACKAGE}`. "
            "That is a broken query, not an empty suite — refusing to report success on a "
            "shard that would interpret nothing."
        )

    # (4) The independent second source of truth: names.
    expected_paths = autodiscovered_paths(root)
    expected = set(expected_paths)
    if set(names) != expected:
        missing = sorted(expected - set(names))
        extra = sorted(set(names) - expected)
        die(
            "`cargo metadata` and `tokora/tests/` disagree about the integration suite.\n"
            f"  metadata reports {len(names)}, the tests directory implies {len(expected)}\n"
            f"  in tokora/tests/ but NOT in metadata (these would go uninterpreted): {missing}\n"
            f"  in metadata but NOT in tokora/tests/: {extra}\n"
            "If a `[[test]]` block with a custom path was added on purpose, teach "
            "`autodiscovered_names` about it — do not delete this check, it is the only thing "
            "that notices the shards quietly covering less than the tree contains."
        )

    # (5) Names agreeing is not files agreeing — see the header. Reject a duplicate metadata
    # name outright: silently keying a dict by `name` would keep only the last one and hide
    # exactly the kind of collision this guard exists to catch.
    meta_paths: dict[str, Path] = {}
    dup_names: set[str] = set()
    for t in test_targets:
        n = t["name"]
        if n in meta_paths:
            dup_names.add(n)
        else:
            meta_paths[n] = Path(t["src_path"])
    if dup_names:
        die(
            "`cargo metadata` reports more than one `test` target named the same: "
            f"{sorted(dup_names)}. Two targets cannot share a name in real cargo output, and "
            "this file cannot pick a canonical `src_path` for either — fix the manifest before "
            "trusting the shard plan."
        )

    mismatches = []
    for name, src_path in sorted(meta_paths.items()):
        resolved = src_path.resolve()
        exception = PATH_EXCEPTIONS.get(name)
        if exception is not None:
            allowed = (root / exception).resolve()
            if resolved == allowed:
                continue
            mismatches.append(
                f"  `{name}`: cargo metadata says {resolved}, but PATH_EXCEPTIONS says "
                f"{allowed} — the exception is stale"
            )
            continue
        want = expected_paths[name].resolve()
        if resolved != want:
            mismatches.append(
                f"  `{name}`: cargo metadata says {resolved}, tokora/tests/ implies {want}"
            )
    if mismatches:
        die(
            "`cargo metadata` and `tokora/tests/` agree on target NAMES but disagree on which "
            "FILE at least one of them builds from. A `[[test]]` block can keep an existing "
            "name while pointing `path` at a different file, and the name-only check in (4) "
            "cannot see that — the coverage the matching name implies would be silently "
            "false:\n" + "\n".join(mismatches) + "\n"
            "If a custom path is intentional, add the target to PATH_EXCEPTIONS with its "
            "approved path — do not delete this check, it is the only thing that notices a "
            "target's content moving out from under its name."
        )

    # (8) The emitted flags are word-split unquoted by the caller.
    bad = [n for n in names if not NAME_RE.match(n)]
    if bad:
        die(
            f"target names outside [A-Za-z0-9_-]: {bad}. The shard flags are word-split by the "
            "calling shell, so these cannot be passed through safely."
        )

    return names


def partition(names: list[str]) -> list[list[str]]:
    """Guard (6)-(7), and the cost-aware assignment itself.

    Greedy LPT (Longest Processing Time first): `names` sorted by DESCENDING
    `TARGET_WEIGHTS` (ties broken by name, so the order — and therefore the whole partition —
    is deterministic regardless of the input list's order), each assigned in turn to whichever
    shard currently carries the LEAST total weight (ties broken by the lower shard index). This
    is the standard approximation algorithm for balanced multiprocessor scheduling; see the
    module header for the measured imbalance it replaces `i % SHARD_TOTAL` to fix.
    """
    # (6) No shard can be empty by arithmetic.
    if SHARD_TOTAL < 1:
        die(f"SHARD_TOTAL must be >= 1, got {SHARD_TOTAL}")
    if SHARD_TOTAL > len(names):
        die(
            f"SHARD_TOTAL is {SHARD_TOTAL} but only {len(names)} integration test targets "
            "exist; some shard would interpret nothing and pass."
        )

    unweighted = sorted(n for n in names if n not in TARGET_WEIGHTS)
    if unweighted:
        note(
            f"{len(unweighted)} of {len(names)} target(s) have no TARGET_WEIGHTS entry and use "
            f"DEFAULT_WEIGHT ({DEFAULT_WEIGHT}s); consider refreshing the table: {unweighted}"
        )

    order = sorted(names, key=lambda n: (-TARGET_WEIGHTS.get(n, DEFAULT_WEIGHT), n))
    shards: list[list[str]] = [[] for _ in range(SHARD_TOTAL)]
    shard_cost = [0.0] * SHARD_TOTAL
    for n in order:
        s = min(range(SHARD_TOTAL), key=lambda i: (shard_cost[i], i))
        shards[s].append(n)
        shard_cost[s] += TARGET_WEIGHTS.get(n, DEFAULT_WEIGHT)
    shards = [sorted(shard) for shard in shards]

    verify_partition(names, shards)
    return shards


def verify_partition(names: list[str], shards: list[list[str]]) -> None:
    """Guard (7): every shard non-empty, the shards pairwise disjoint, and their union equal to
    `names`. Split out of `partition()` so the selftest can drive it directly against a
    deliberately broken `shards` list — an empty shard, a duplicate, an omitted or a phantom
    name — without needing a broken assignment algorithm to produce one. Each of the four checks
    below is independent and ordered so that a case built to isolate one of them cannot also
    trip an earlier one first: see `mode_selftest` for the fixtures that exercise each in
    isolation.

    `names` MUST already be sorted, the way `enumerate_targets` documents its return value —
    the union check compares `sorted(flat) != names` and a `names` that is not itself sorted
    fails that comparison regardless of whether the partition is actually complete. The one
    real caller, `partition()`, always satisfies this; a hand-built `names` list passed here
    directly (as the selftest's guard (7) fixtures do) must be sorted too.
    """
    for s, shard in enumerate(shards):
        if not shard:
            die(f"shard {s} of {len(shards)} enumerated zero targets")

    flat = [n for shard in shards for n in shard]
    if len(flat) != len(names):
        die(
            f"partition size {len(flat)} != enumerated size {len(names)}: the shards do not "
            "account for the suite exactly once"
        )
    dupes = sorted({n for n in flat if flat.count(n) > 1})
    if dupes:
        die(f"targets assigned to more than one shard: {dupes}")
    if sorted(flat) != names:
        omitted = sorted(set(names) - set(flat))
        die(f"the union of the shards is not the enumerated list; omitted: {omitted}")


def emit_flags(shard: int, unit: str, shards: list[list[str]]) -> None:
    if unit == "doc":
        if shard == DOC_SHARD:
            note(f"shard {shard}/{SHARD_TOTAL} owns the doctests")
            print("--doc")
        else:
            note(f"shard {shard}/{SHARD_TOTAL} does not own the doctests")
            print("SKIP")
        return

    if unit == "logos":
        lib = shard == LIB_LOGOS_SHARD
    elif unit == "raw":
        lib = shard == LIB_RAW_SHARD
    else:
        die(f"unknown unit `{unit}`; expected one of: logos, raw, doc")

    slice_ = shards[shard]
    # Guard (8) already proved every name matches `[A-Za-z0-9_-]+`, but a name starting with
    # `-` — e.g. a target literally named `--help` — is still a legal match for that regex. If
    # `--test` and the name were emitted as two separate words, the calling shell's word-split
    # hands cargo `--test --help` as two argv entries; clap reads the bare `--help` as its own
    # global flag rather than as `--test`'s value, and cargo prints help and exits 0 — a shard
    # that ran nothing, green. Joining them into one word, `--test=<name>`, removes the
    # ambiguity entirely: clap always treats the text after `=` as the value, whatever it
    # starts with.
    flags = (["--lib"] if lib else []) + [f"--test={n}" for n in slice_]
    note(
        f"shard {shard}/{SHARD_TOTAL} unit={unit}: {len(slice_)} integration targets, "
        f"lib={'yes' if lib else 'no'}"
    )
    print(" ".join(flags))


def mode_plan(names: list[str], shards: list[list[str]]) -> None:
    print(f"integration test targets enumerated: {len(names)}")
    print(f"shards: {SHARD_TOTAL}")
    print(
        f"pinned: lib(logos)=shard {LIB_LOGOS_SHARD}, doctests=shard {DOC_SHARD}, "
        f"lib(logos,unstable-raw)=shard {LIB_RAW_SHARD}"
    )
    for s, shard in enumerate(shards):
        pinned = []
        if s == LIB_LOGOS_SHARD:
            pinned.append("lib(logos)")
        if s == DOC_SHARD:
            pinned.append("doc")
        if s == LIB_RAW_SHARD:
            pinned.append("lib(raw)")
        extra = f"  + {', '.join(pinned)}" if pinned else ""
        print(f"  shard {s}: {len(shard):3d} targets{extra}")
    print(f"union verified: {sum(len(s) for s in shards)} == {len(names)}, no duplicates")

    out = os.environ.get("GITHUB_OUTPUT")
    payload = json.dumps(list(range(SHARD_TOTAL)))
    if out:
        with open(out, "a", encoding="utf-8") as fh:
            fh.write(f"shards={payload}\n")
        note(f"emitted shards={payload} to $GITHUB_OUTPUT")
    else:
        note(f"$GITHUB_OUTPUT unset; shards would be {payload}")


def mode_selftest() -> None:
    """Regression tests for every testable guard: (2)-(8) except (1), which needs a real
    `cargo metadata` invocation to fail and is exempt by design (see the module header). Run
    against mocked `cargo metadata` JSON, hand-built shard lists for `verify_partition`, and a
    throwaway `tests/` tree. Never shells out to cargo, never touches the real manifest or the
    real `tokora/tests/`.

    In the spirit of `ci/changelog_structure_selftest.sh`: a green case proves the guard does
    not refuse by default, and each red case proves the SPECIFIC guard fires — not just that
    something, somewhere, died. Exits non-zero if any case fails.
    """
    import shutil
    import tempfile
    from io import StringIO

    results: list[tuple[str, bool, str]] = []

    def check(name: str, ok: bool, detail: str = "") -> None:
        results.append((name, ok, detail))

    def run(fn, *args) -> tuple[int, str]:
        """Calls `fn`, trapping the `die()` exit path. Returns (exit_code, stderr)."""
        old_err = sys.stderr
        sys.stderr = captured = StringIO()
        try:
            fn(*args)
            code = 0
        except SystemExit as exc:
            code = 1 if exc.code else 0
        finally:
            sys.stderr = old_err
        return code, captured.getvalue()

    def run_stdout(fn, *args) -> tuple[int, str, str]:
        """Like `run`, but also captures stdout."""
        old_out = sys.stdout
        sys.stdout = captured_out = StringIO()
        try:
            code, err = run(fn, *args)
        finally:
            sys.stdout = old_out
        return code, captured_out.getvalue(), err

    # A throwaway `<tmp>/tokora/tests/` tree — never the real one — resolved up front so every
    # path compared against it later is already canonical and string-identical.
    tmp = Path(tempfile.mkdtemp(prefix="miri_shard_selftest_")).resolve()
    try:
        tests_dir = tmp / PACKAGE / "tests"
        tests_dir.mkdir(parents=True)
        # A subdirectory with no `main.rs` is invisible to `autodiscovered_paths` — exactly
        # like the real `tokora/tests/common/` — so a `[[test]]` block can point here without
        # ever changing the autodiscovered name set. This is what makes the Finding-1 exploit
        # invisible to guard (4) and visible only to guard (5).
        support_dir = tests_dir / "support"
        support_dir.mkdir()
        foo = tests_dir / "foo.rs"
        bar = tests_dir / "bar.rs"
        decoy = support_dir / "decoy.rs"
        for f in (foo, bar, decoy):
            f.write_text("// miri_shard selftest fixture\n", encoding="utf-8")

        def meta_with_foo_at(foo_src: Path) -> dict:
            return {
                "packages": [
                    {
                        "name": PACKAGE,
                        "targets": [
                            {"name": "foo", "kind": ["test"], "src_path": str(foo_src)},
                            {"name": "bar", "kind": ["test"], "src_path": str(bar)},
                        ],
                    }
                ]
            }

        # RED — guard (2): zero packages named `tokora`. A stale package filter (e.g. after a
        # workspace rename) must not read as "a package with no tests" — it must name the count
        # it actually found instead.
        meta_no_pkg = {"packages": [{"name": "not-tokora", "targets": []}]}
        code, err = run(verify_targets, meta_no_pkg, tmp)
        check(
            "guard(2) red: zero packages named tokora is refused",
            code != 0 and "exactly one workspace package" in err and "found 0" in err,
            err,
        )

        # RED — guard (2): two packages named `tokora`. Cannot happen from real cargo output,
        # but the guard counts rather than assumes, so a mock can still drive it.
        meta_two_pkg = {
            "packages": [
                {"name": PACKAGE, "targets": []},
                {"name": PACKAGE, "targets": []},
            ]
        }
        code, err = run(verify_targets, meta_two_pkg, tmp)
        check(
            "guard(2) red: two packages named tokora is refused",
            code != 0 and "exactly one workspace package" in err and "found 2" in err,
            err,
        )

        # RED — guard (3): the one `tokora` package exists but declares zero `test`-kind
        # targets. The positive control the header names: without this, "the query matched
        # nothing" and "the crate has no integration tests" render as the same green.
        meta_no_tests = {
            "packages": [
                {
                    "name": PACKAGE,
                    "targets": [{"name": "tokora", "kind": ["lib"], "src_path": str(foo)}],
                }
            ]
        }
        code, err = run(verify_targets, meta_no_tests, tmp)
        check(
            "guard(3) red: a package with zero test-kind targets is refused",
            code != 0 and "ZERO integration test targets" in err,
            err,
        )

        # RED — guard (4): metadata omits a target the tree implies (`bar`). `bar` is never a
        # path-comparison candidate here, so this cannot be guard (5) firing instead.
        meta_missing_bar = {
            "packages": [
                {
                    "name": PACKAGE,
                    "targets": [{"name": "foo", "kind": ["test"], "src_path": str(foo)}],
                }
            ]
        }
        code, err = run(verify_targets, meta_missing_bar, tmp)
        check(
            "guard(4) red: metadata missing a tree-implied target is refused",
            code != 0 and "disagree about the integration suite" in err and "'bar'" in err,
            err,
        )

        # RED — guard (4): metadata names a target the tree does not imply at all (`ghost` is
        # never written under `tests_dir`). The complementary direction from the case above.
        meta_extra_ghost = {
            "packages": [
                {
                    "name": PACKAGE,
                    "targets": [
                        {"name": "foo", "kind": ["test"], "src_path": str(foo)},
                        {"name": "bar", "kind": ["test"], "src_path": str(bar)},
                        {"name": "ghost", "kind": ["test"], "src_path": str(foo)},
                    ],
                }
            ]
        }
        code, err = run(verify_targets, meta_extra_ghost, tmp)
        check(
            "guard(4) red: metadata naming a target absent from the tree is refused",
            code != 0 and "disagree about the integration suite" in err and "'ghost'" in err,
            err,
        )

        # GREEN — names AND paths agree with tokora/tests/: guard (5) must not refuse this.
        code, err = run(verify_targets, meta_with_foo_at(foo), tmp)
        check("guard(5) green: matching src_path is accepted", code == 0, err)

        # RED — Finding 1: `foo`'s metadata name is unchanged but its `src_path` now points at
        # `support/decoy.rs`. Guard (4)'s name-set comparison alone cannot see this (both sides
        # still say just {foo, bar}); guard (5) must refuse it, naming the target and both
        # paths.
        code, err = run(verify_targets, meta_with_foo_at(decoy), tmp)
        check(
            "guard(5) red: src_path mismatch under an unchanged name is refused",
            code != 0 and "foo" in err and str(decoy) in err,
            err,
        )

        # RED: two metadata targets sharing one name. Keying a dict by name would silently keep
        # only the last and hide the collision; this must be refused instead.
        dup_meta = {
            "packages": [
                {
                    "name": PACKAGE,
                    "targets": [
                        {"name": "foo", "kind": ["test"], "src_path": str(foo)},
                        {"name": "foo", "kind": ["test"], "src_path": str(decoy)},
                        {"name": "bar", "kind": ["test"], "src_path": str(bar)},
                    ],
                }
            ]
        }
        code, err = run(verify_targets, dup_meta, tmp)
        check(
            "guard(5) red: duplicate metadata target name is refused",
            code != 0 and "foo" in err and "more than one" in err,
            err,
        )

        # GREEN: a declared PATH_EXCEPTIONS entry — and only that — makes a real path
        # difference pass, and it is undone immediately after so it cannot leak into any other
        # case, or worse, into a real `plan`/`flags` invocation later in the same process.
        saved = dict(PATH_EXCEPTIONS)
        try:
            PATH_EXCEPTIONS.clear()
            PATH_EXCEPTIONS["foo"] = str(decoy.relative_to(tmp))
            code, err = run(verify_targets, meta_with_foo_at(decoy), tmp)
            check("guard(5) green: a declared PATH_EXCEPTIONS entry is honoured", code == 0, err)
        finally:
            PATH_EXCEPTIONS.clear()
            PATH_EXCEPTIONS.update(saved)

        # RED — this finding (Codex round 2): every case above drives guard (8) only through
        # `emit_flags`, with mocked shard slices it invents itself — never through
        # `verify_targets`, the only place guard (8) actually runs. A name can satisfy guards
        # (4) and (5) — cargo metadata's name matches the tree, and the tree's file is the one
        # metadata claims to be building — and still be shell-significant text that must never
        # reach a word-split shell as a bare word; guard (8) is the only thing that refuses it.
        # A fresh tree is used so these bad names never join `expected`/`meta_with_foo_at` for
        # the guard(5) cases above.
        badname_tmp = Path(tempfile.mkdtemp(prefix="miri_shard_selftest_badname_")).resolve()
        try:
            badname_tests_dir = badname_tmp / PACKAGE / "tests"
            badname_tests_dir.mkdir(parents=True)

            def meta_with_only(name: str, src: Path) -> dict:
                return {
                    "packages": [
                        {
                            "name": PACKAGE,
                            "targets": [{"name": name, "kind": ["test"], "src_path": str(src)}],
                        }
                    ]
                }

            # Whitespace and a glob character: word-splitting and globbing are the two shell
            # behaviours guard (8) exists to keep an unquoted `--test=<name>` safe against. Each
            # is written as its own file and removed before the next so the tree — and therefore
            # `expected` — only ever contains the one name under test.
            for bad_name in ("foo bar", "foo*bar"):
                bad_file = badname_tests_dir / f"{bad_name}.rs"
                bad_file.write_text("// miri_shard selftest fixture\n", encoding="utf-8")
                try:
                    code, err = run(verify_targets, meta_with_only(bad_name, bad_file), badname_tmp)
                    check(
                        f"guard(8) red: verify_targets refuses name {bad_name!r} even though "
                        "guards (4) and (5) both agree it is the tree's real name",
                        code != 0 and "outside [A-Za-z0-9_-]" in err and bad_name in err,
                        err,
                    )
                finally:
                    bad_file.unlink()
        finally:
            shutil.rmtree(badname_tmp, ignore_errors=True)

        # RED — Finding 2: a target named `--help` must never reach the shell as `--test`
        # followed by a bare word starting with `-`; that bare word is cargo's own help flag
        # and would exit 0 having run nothing.
        _, out, _ = run_stdout(emit_flags, 0, "raw", [["--help"]])
        check(
            "emit_flags red: a leading-dash name is fused to --test=, not a bare word",
            "--test --help" not in out and "--test=--help" in out,
            out,
        )

        # GREEN: ordinary names still join the way cargo expects — no regression from the
        # `--test=<name>` change.
        _, out, _ = run_stdout(emit_flags, 0, "raw", [["alpha", "beta"]])
        check(
            "emit_flags green: ordinary names still join as --test=<name>",
            out.split() == ["--test=alpha", "--test=beta"],
            out,
        )

        # ── guard (6): SHARD_TOTAL bounds, driven through partition() ───────────────────────
        # `partition()` reads the module-level SHARD_TOTAL directly (never as a parameter), so
        # these three cases save and restore it exactly like the PATH_EXCEPTIONS cases above
        # save and restore that global — never left mutated for a case after this one, or for a
        # real `plan`/`flags` call later in the same process.
        global SHARD_TOTAL
        saved_shard_total = SHARD_TOTAL
        try:
            SHARD_TOTAL = 0
            code, err = run(partition, ["a", "b"])
            check(
                "guard(6) red: SHARD_TOTAL < 1 is refused",
                code != 0 and "SHARD_TOTAL must be >= 1" in err,
                err,
            )

            SHARD_TOTAL = 5
            code, err = run(partition, ["a", "b"])
            check(
                "guard(6) red: SHARD_TOTAL greater than the target count is refused",
                code != 0 and "only 2 integration test targets exist" in err,
                err,
            )

            SHARD_TOTAL = 2
            code, err = run(partition, ["a", "b", "c", "d"])
            check("guard(6) green: SHARD_TOTAL within bounds is accepted", code == 0, err)
        finally:
            SHARD_TOTAL = saved_shard_total

        # ── guard (7): verify_partition, driven directly with a deliberately broken `shards` ──
        # `partition()` itself can never produce most of these — LPT cannot emit an empty
        # shard, a duplicate or a phantom name from valid input — so each fixture below is
        # hand-built to reach exactly one of guard (7)'s four checks, in an order that proves
        # the earlier checks in `verify_partition` do not fire first and mask it.
        #
        # MUST be alphabetically sorted (see verify_partition's docstring) — an unsorted names4
        # would fail the union check even for a genuinely complete partition, for a reason
        # having nothing to do with coverage. This was caught by the green case below going red
        # for the wrong reason on the first version of this fixture.
        names4 = ["alpha", "beta", "delta", "gamma"]

        # GREEN: a valid, hand-built partition must not be refused.
        code, err = run(verify_partition, names4, [["alpha", "beta"], ["gamma", "delta"]])
        check("guard(7) green: a correct partition is accepted", code == 0, err)

        # RED: an empty shard.
        code, err = run(verify_partition, names4, [["alpha", "beta", "gamma", "delta"], []])
        check(
            "guard(7) red: an empty shard is refused",
            code != 0 and "enumerated zero targets" in err,
            err,
        )

        # RED: undercount with no duplicate (one shard silently drops `delta`) — isolates the
        # size check from the duplicate check below it: no shard is empty, no name repeats.
        code, err = run(verify_partition, names4, [["alpha", "beta"], ["gamma"]])
        check(
            "guard(7) red: a shard list smaller than the enumerated list is refused",
            code != 0 and "partition size 3 != enumerated size 4" in err,
            err,
        )

        # RED: a duplicate, sized to still equal len(names4) so the size check above does not
        # fire first — `beta` appears twice, `delta` not at all.
        code, err = run(verify_partition, names4, [["alpha", "beta"], ["beta", "gamma"]])
        check(
            "guard(7) red: a name assigned to two shards is refused",
            code != 0 and "beta" in err and "more than one shard" in err,
            err,
        )

        # RED: a phantom name outside names4, standing in for a dropped real name (`delta`),
        # sized and duplicate-free so only the union-equality check can fire.
        code, err = run(verify_partition, names4, [["alpha", "beta"], ["gamma", "phantom"]])
        check(
            "guard(7) red: a name outside the enumerated list is refused",
            code != 0
            and "union of the shards is not the enumerated list" in err
            and "delta" in err,
            err,
        )
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    failed = [name for name, ok, _ in results if not ok]
    for name, ok, detail in results:
        note(f"selftest: {'OK  ' if ok else 'FAIL'} {name}")
        if not ok and detail:
            note(f"selftest:      {detail.strip()[:400]}")

    if failed:
        die(f"selftest: {len(failed)}/{len(results)} case(s) failed: {failed}")
    note(f"selftest: {len(results)}/{len(results)} case(s) passed")


def main(argv: list[str]) -> None:
    if len(argv) < 2:
        die(
            "usage: miri_shard.py plan | miri_shard.py flags <shard> <logos|raw|doc> | "
            "miri_shard.py selftest"
        )

    mode = argv[1]

    if mode == "selftest":
        if len(argv) != 2:
            die("usage: miri_shard.py selftest")
        mode_selftest()
        return

    # The guard runs in every remaining mode, before anything is emitted. There is no path
    # through `plan` or `flags` that reaches a cargo invocation without it. `selftest` is
    # exempt on purpose: it supplies its own mocked metadata and must not depend on — or
    # perturb — the real repository's `cargo metadata` output.
    names = enumerate_targets()
    shards = partition(names)

    if mode == "plan":
        if len(argv) != 2:
            die("usage: miri_shard.py plan")
        mode_plan(names, shards)
        return

    if mode == "flags":
        if len(argv) != 4:
            die("usage: miri_shard.py flags <shard> <logos|raw|doc>")
        try:
            shard = int(argv[2])
        except ValueError:
            die(f"shard index `{argv[2]}` is not an integer")
        if not 0 <= shard < SHARD_TOTAL:
            die(f"shard index {shard} outside 0..{SHARD_TOTAL - 1}")
        emit_flags(shard, argv[3], shards)
        return

    die(f"unknown mode `{mode}`; expected `plan`, `flags`, or `selftest`")


if __name__ == "__main__":
    main(sys.argv)
