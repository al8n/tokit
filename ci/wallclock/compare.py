#!/usr/bin/env python3
"""Fold one run's interleaved wall-clock readings into a per-id verdict.

Input is the JSONL `ci/wallclock/measure.py` appended to, one line per (round, target, side).
Both sides were built and measured in the SAME job, on the SAME runner, with the SAME toolchain
— see `ci/wallclock/run.sh` for why that is the design and not an implementation detail.

# min over rounds, of the min over samples

Each line already carries the minimum per-iteration time criterion observed within that round.
This folds those with `min` again across rounds, which is the same operator applied to the union
of every sample the side produced. Noise is one-directional, so the minimum is the estimator that
converges to the true cost rather than to the cost plus the job's noise integral.

`min` is order-independent, so the interleaving `run.sh` performs buys nothing *arithmetically*.
What it buys is that the two sides' sample populations are drawn from the same wall-clock window.
A neighbouring container that arrives at minute six and stays lands on the second half of both
sides' rounds instead of on the whole of one of them, and each side still has an early clean
round for its minimum to come from.

# The threshold is a property of the runner, not a round number

`run.sh --self` builds the same commit twice and measures both, which is a full pass of this
pipeline over a difference that is known to be zero. Whatever spread that reports IS this gate's
floor, and the threshold has to sit above it — above the MAXIMUM over ids, not above the typical
id, because a job that fails when any one of forty-six rows exceeds the threshold fails at
roughly forty-six times a single row's rate.

Usage:
    compare.py <readings.jsonl> --threshold PCT [--accept-file FILE] [--summary FILE]
                               [--self-comparison] [--short-id-floor-ns NS]
"""

import argparse
import collections
import json
import os
import re
import sys

# `Perf-accept: <id> +<pct>% <reason>` — the icount gate's trailer, verbatim in grammar and in
# meaning, with one widening: a criterion id is `parser/repeated_collect`, not `sep_while`, so the
# name charset admits `/`, `-` and `.`. Everything else is deliberately identical, including that
# an entry with no reason is refused and that the widest ceiling wins. A branch that has to accept
# a trade in both gates writes two trailers of the same shape rather than learning a second
# convention.
ACCEPT_RE = re.compile(
    r"^\s*Perf-accept:\s*(?P<id>[A-Za-z0-9_./-]+)\s*\+?(?P<pct>[0-9]+(?:\.[0-9]+)?)\s*%"
    r"\s*(?P<reason>\S.*?)\s*$"
)

# Below this, an id's per-iteration time is close enough to the clock and to criterion's own
# per-sample overhead that a percentage of it is not a measurement. Nothing in this repository is
# anywhere near it today — the shortest of the forty-six is tens of microseconds — so this is a
# tripwire for a bench added later, not a filter that removes anything. It REPORTS rather than
# drops: an id this gate cannot read is a hole in the population and has to be visible as one.
DEFAULT_SHORT_ID_FLOOR_NS = 1_000.0


def parse_acceptances(path):
    """Read `Perf-accept:` trailers, one per line, as harvested from the branch's commits.

    An entry with no reason is REFUSED rather than ignored, for the reason the icount gate gives:
    an acceptance is a statement that a trade was worth making, and an acceptance with nothing
    said about the trade is the shape that turns a gate into a rubber stamp.
    """
    accepted = {}
    if not path or not os.path.exists(path):
        return accepted
    with open(path, encoding="utf-8") as fh:
        for lineno, line in enumerate(fh, 1):
            if not line.strip():
                continue
            m = ACCEPT_RE.match(line)
            if not m:
                sys.exit(
                    f"::error::{path}:{lineno}: cannot read this as an acceptance:\n"
                    f"    {line.strip()}\n"
                    "  expected: Perf-accept: <id> +<pct>% <why it is worth it>"
                )
            name, pct, reason = m["id"], float(m["pct"]), m["reason"]
            if name not in accepted or pct > accepted[name][0]:
                accepted[name] = (pct, reason)
    return accepted


def load(path):
    """Read the JSONL into {side: {id: [per-round ns]}} plus per-side bookkeeping."""
    per_round = {"base": collections.defaultdict(list), "head": collections.defaultdict(list)}
    spread = {"base": collections.defaultdict(list), "head": collections.defaultdict(list)}
    rounds = collections.defaultdict(set)
    seconds = 0.0
    targets = set()
    with open(path, encoding="utf-8") as fh:
        for lineno, line in enumerate(fh, 1):
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError as exc:
                sys.exit(f"::error::{path}:{lineno}: {exc}")
            side = rec["side"]
            rounds[side].add(rec["round"])
            targets.add(rec["target"])
            seconds += rec.get("seconds", 0.0)
            for name, reading in rec["ids"].items():
                per_round[side][name].append(reading["ns_per_iter"])
                spread[side][name].append(reading["sample_spread_pct"])
    return per_round, spread, rounds, targets, seconds


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("readings")
    ap.add_argument("--threshold", type=float, required=True)
    ap.add_argument("--accept-file")
    ap.add_argument("--summary", help="append a markdown table here (GITHUB_STEP_SUMMARY)")
    ap.add_argument(
        "--self-comparison",
        action="store_true",
        help="both sides are the same commit; report the floor instead of a verdict",
    )
    ap.add_argument("--short-id-floor-ns", type=float, default=DEFAULT_SHORT_ID_FLOOR_NS)
    args = ap.parse_args()

    per_round, spread, rounds, targets, seconds = load(args.readings)

    if not per_round["base"] or not per_round["head"]:
        sys.exit(
            "::error::one side produced no readings at all "
            f"(base ids {len(per_round['base'])}, head ids {len(per_round['head'])})"
        )
    if rounds["base"] != rounds["head"]:
        sys.exit(
            f"::error::the two sides ran different rounds ({sorted(rounds['base'])} vs "
            f"{sorted(rounds['head'])}) — they are not comparable"
        )
    if set(per_round["base"]) != set(per_round["head"]):
        only_base = sorted(set(per_round["base"]) - set(per_round["head"]))
        only_head = sorted(set(per_round["head"]) - set(per_round["base"]))
        sys.exit(
            "::error::the two sides measured different ids — the instrument is not the same on "
            f"both.\n  only in base: {only_base}\n  only in head: {only_head}"
        )
    n_rounds = len(rounds["head"])
    for side in ("base", "head"):
        for name, values in per_round[side].items():
            if len(values) != n_rounds:
                sys.exit(
                    f"::error::{name} was read {len(values)} times on the {side} side but there "
                    f"were {n_rounds} rounds — a round is missing from the readings"
                )

    accepted = parse_acceptances(args.accept_file)

    rows = []
    for name in per_round["head"]:
        b = min(per_round["base"][name])
        h = min(per_round["head"][name])
        pct = (h - b) / b * 100.0
        # The runner's own noise, measured on this id in this job: how much the SAME build's
        # per-round minima moved across the rounds. This is the number the threshold answers to.
        noise = max(
            (max(per_round[side][name]) / min(per_round[side][name]) - 1.0) * 100.0
            for side in ("base", "head")
        )
        ceiling, reason = accepted.get(name, (None, None))
        if pct <= args.threshold:
            verdict = "ok"
        elif ceiling is not None and pct <= ceiling:
            verdict = f"accepted (<= +{ceiling:g}%)"
        else:
            verdict = "REGRESSION"
        rows.append(
            {
                "id": name,
                "base_ns": b,
                "head_ns": h,
                "pct": pct,
                "noise_pct": noise,
                "verdict": verdict,
                "reason": reason,
                "short": min(b, h) < args.short_id_floor_ns,
            }
        )

    rows.sort(key=lambda r: -r["pct"])
    width = max(len(r["id"]) for r in rows)

    print()
    print(
        f"{'id':<{width}}  {'base ns/iter':>13}  {'head ns/iter':>13}  {'delta':>9}  "
        f"{'own noise':>9}  verdict"
    )
    print("-" * (width + 62))
    for r in rows:
        print(
            f"{r['id']:<{width}}  {r['base_ns']:>13,.0f}  {r['head_ns']:>13,.0f}  "
            f"{r['pct']:>+8.2f}%  {r['noise_pct']:>8.2f}%  {r['verdict']}"
        )
        if r["reason"] and r["verdict"].startswith("accepted"):
            print(f"{'':<{width}}  {'':>13}  {'':>13}  {'':>9}  {'':>9}  reason: {r['reason']}")

    deltas = [r["pct"] for r in rows]
    noises = [r["noise_pct"] for r in rows]
    worst_noise = max(noises)
    print()
    print(
        f"{len(rows)} ids over {len(targets)} bench targets, {n_rounds} interleaved rounds a "
        f"side, {seconds / 60.0:.1f} minutes of measurement."
    )
    print(
        f"round-to-round spread of the same build: worst id {worst_noise:.2f}%, "
        f"median id {sorted(noises)[len(noises) // 2]:.2f}%."
    )
    print(
        f"observed deltas: {min(deltas):+.2f}% to {max(deltas):+.2f}%, against a threshold of "
        f"+{args.threshold:g}%."
    )

    short = [r for r in rows if r["short"]]
    if short:
        # Stated, never dropped: an id this instrument cannot read is a hole in the population,
        # and a hole nothing prints is a hole nobody knows about.
        print()
        for r in short:
            print(
                f"::warning::`{r['id']}` runs in {min(r['base_ns'], r['head_ns']):,.0f} ns an "
                f"iteration, below this gate's {args.short_id_floor_ns:,.0f} ns floor. A "
                "percentage of it is closer to the clock's resolution than to a measurement; "
                "read its row as advisory even by this gate's standards."
            )

    unmatched = sorted(set(accepted) - {r["id"] for r in rows})
    if unmatched:
        # NOT an error. `Perf-accept:` is shared with the instruction-count gate, whose rows are
        # `tokora-icount` workload names, so a trailer this gate does not recognise is very
        # probably addressed to the other one. Saying so beats both silence and a false alarm.
        print()
        for name in unmatched:
            print(
                f"note: the branch carries `Perf-accept: {name}` and this gate measures no id of "
                "that name. If it was meant for the instruction-count gate, nothing is wrong; if "
                "it was meant for this one, the id is misspelled and licenses nothing."
            )

    if args.self_comparison:
        # The calibration run. There is no regression to find — both sides are the same commit —
        # so the interesting quantity is the spread, and the verdict is about the THRESHOLD.
        worst = max(abs(d) for d in deltas)
        print()
        print(
            f"SELF-COMPARISON: the same commit built twice and measured twice. The largest "
            f"absolute difference over {len(rows)} ids is {worst:.2f}%; that is this runner's "
            "floor for this configuration, and any threshold at or below it fails at random."
        )
        print(
            f"suggested threshold: +{max(1.0, round(worst * 2.0, 1)):g}% — twice the observed "
            "floor, which is the margin the floor's own run-to-run variation needs."
        )
        if worst > args.threshold:
            print()
            print(
                f"::error::the self-comparison spans {worst:.2f}%, which exceeds the configured "
                f"threshold of +{args.threshold:g}%. As configured this gate reds on unchanged "
                "code. Re-derive WALL_THRESHOLD from this run before trusting a verdict from it."
            )
            return 1
        print(
            f"the configured threshold of +{args.threshold:g}% clears the floor by "
            f"{args.threshold - worst:.2f} points."
        )
        return 0

    if worst_noise > args.threshold:
        # The gate's own instrument disagreeing with its threshold, printed whatever the verdict:
        # a row whose SAME-BUILD readings move further than the threshold cannot support a claim
        # about the other build, in either direction.
        print()
        print(
            f"::warning::the noisiest id moved {worst_noise:.2f}% between rounds of the SAME "
            f"build, which is more than the +{args.threshold:g}% threshold. On this runner, in "
            "this job, a verdict at that threshold is at the edge of what the measurement "
            "supports; treat a single row just over the line as unproven and re-run."
        )

    failed = [r for r in rows if r["verdict"] == "REGRESSION"]

    if len(failed) == len(rows) and len(rows) > 1:
        print()
        print(
            "note: EVERY id regressed, including targets that share no code path — `cst` builds "
            "a tree from pre-made events and reaches no parser at all. A change that moved all "
            "of them is far more likely to be the runner (a slower host, a noisy neighbour, a "
            "different CPU model between the two halves of the job) than the branch. Re-run "
            "before reading this as a regression."
        )

    if args.summary:
        with open(args.summary, "a", encoding="utf-8") as fh:
            fh.write("### wall clock vs merge-base (advisory)\n\n")
            fh.write("| id | base ns/iter | head ns/iter | delta | own noise | verdict |\n")
            fh.write("| --- | ---: | ---: | ---: | ---: | --- |\n")
            for r in rows:
                note = (
                    f" — {r['reason']}" if r["reason"] and r["verdict"].startswith("accepted")
                    else ""
                )
                fh.write(
                    f"| `{r['id']}` | {r['base_ns']:,.0f} | {r['head_ns']:,.0f} | "
                    f"{r['pct']:+.2f}% | {r['noise_pct']:.2f}% | {r['verdict']}{note} |\n"
                )
            fh.write(
                f"\nThreshold **+{args.threshold:g}%**, min of {n_rounds} interleaved rounds a "
                f"side; both sides built and measured in this run on this runner. Worst "
                f"same-build round-to-round spread this job: **{worst_noise:.2f}%**.\n"
            )

    if not failed:
        print("\nwallclock: no id regressed past the threshold.")
        return 0

    print()
    for r in failed:
        print(
            f"::error::{r['id']} is {r['pct']:+.2f}% slower than the merge-base: "
            f"{r['base_ns']:,.0f} -> {r['head_ns']:,.0f} ns per iteration, against a threshold "
            f"of +{args.threshold:g}% and a same-build round-to-round spread of "
            f"{r['noise_pct']:.2f}% on this row."
        )
    print(
        "\nThis gate is COARSE. It sees cache cliffs, allocation added to a hot loop and data "
        "movement — the costs an instruction count is blind to — and it cannot resolve the few "
        "percent the `instruction counts` job resolves. Read a failure here as 'something got "
        "materially slower', then use that job to say what.\n"
        "\nTo see it locally:\n"
        "    ci/wallclock/run.sh <the merge-base sha>\n"
        "\nIf the extra time buys something worth having, record the trade in a commit message "
        "on this branch — one trailer per id, the same convention the instruction-count gate "
        "uses:\n"
    )
    for r in failed:
        ceiling = max(1.0, round(r["pct"] + args.threshold))
        print(f"    Perf-accept: {r['id']} +{ceiling:g}% <why the extra time is worth it>")
    print(
        "\nThe trailer is read from this branch's own commits, so it expires when they are no "
        "longer this branch's: it licenses one change and does not become a standing allowance "
        "for the id. `git commit --allow-empty` is enough to add one."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
