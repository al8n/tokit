#!/usr/bin/env python3
"""Compare two instruction-count readings and decide whether the head side regressed.

Both readings come from the SAME CI run, the SAME runner and the SAME toolchain — see
`ci/icount/run.sh` for why that is the whole design and not an implementation detail.

Usage:
    compare.py <base.json> <head.json> --threshold PCT [--accept-file FILE] [--summary FILE]
"""

import argparse
import json
import os
import re
import sys

# The workload that is not an axis. It reaches no repetition engine, so a delta in it is a
# scanner or lexer change — which is what makes it the first thing to read when every axis
# moves at once.
CONTROL = "scan_drain"

# Three axes are measured twice: once over few long collections and once over many short ones.
# The suffix is how the pair is found, DERIVED from the names the binary declared rather than
# written down here — a table of pairs is a table that stops matching the day a workload is
# renamed, and stops silently, because a note that does not fire looks exactly like a note that
# had nothing to say. `tokora-icount/src/workloads.rs` explains what the two depths are for.
SHALLOW_SUFFIX = "_shallow"

# `Perf-accept: <workload> +<pct>% <reason>`
ACCEPT_RE = re.compile(
    r"^\s*Perf-accept:\s*(?P<workload>[A-Za-z0-9_]+)\s*\+?(?P<pct>[0-9]+(?:\.[0-9]+)?)\s*%"
    r"\s*(?P<reason>\S.*?)\s*$"
)


def load(path):
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def parse_acceptances(path):
    """Read `Perf-accept:` trailers, one per line, as harvested from the branch's commits.

    An entry with no reason is REFUSED rather than ignored. An acceptance is a statement that
    a trade was worth making, and an acceptance with nothing said about the trade is the shape
    that turns this gate into a rubber stamp.
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
                    "  expected: Perf-accept: <workload> +<pct>% <why it is worth it>"
                )
            name, pct, reason = m["workload"], float(m["pct"]), m["reason"]
            # The widest ceiling wins when a branch carries more than one for a workload: the
            # later commit is the one that knew what the earlier ones cost.
            if name not in accepted or pct > accepted[name][0]:
                accepted[name] = (pct, reason)
    return accepted


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("base")
    ap.add_argument("head")
    ap.add_argument("--threshold", type=float, required=True)
    ap.add_argument("--accept-file")
    ap.add_argument("--summary", help="append a markdown table here (GITHUB_STEP_SUMMARY)")
    args = ap.parse_args()

    base, head = load(args.base), load(args.head)

    # The two sides must have been read the same way, or the difference is not a difference.
    for field in ("iters_lo", "iters_hi"):
        if base[field] != head[field]:
            sys.exit(
                f"::error::the two sides were read at different {field} "
                f"({base[field]} vs {head[field]}) — they are not comparable"
            )
    if set(base["workloads"]) != set(head["workloads"]):
        only_base = sorted(set(base["workloads"]) - set(head["workloads"]))
        only_head = sorted(set(head["workloads"]) - set(base["workloads"]))
        sys.exit(
            "::error::the two sides measured different workloads — the instrument is not the "
            f"same on both.\n  only in base: {only_base}\n  only in head: {only_head}"
        )

    accepted = parse_acceptances(args.accept_file)
    span = head["iters_hi"] - head["iters_lo"]

    rows = []
    for name in head["workloads"]:
        b = base["workloads"][name]["ir_delta"]
        h = head["workloads"][name]["ir_delta"]
        pct = (h - b) / b * 100.0
        ceiling, reason = accepted.get(name, (None, None))
        if pct <= args.threshold:
            verdict = "ok"
        elif ceiling is not None and pct <= ceiling:
            verdict = f"accepted (<= +{ceiling:g}%)"
        else:
            verdict = "REGRESSION"
        rows.append((name, b / span, h / span, pct, verdict, reason, ceiling))

    rows.sort(key=lambda r: -r[3])

    width = max(len(r[0]) for r in rows)
    print()
    print(f"{'workload':<{width}}  {'base Ir/iter':>14}  {'head Ir/iter':>14}  {'delta':>9}  verdict")
    print("-" * (width + 60))
    for name, b, h, pct, verdict, reason, _ in rows:
        print(f"{name:<{width}}  {b:>14,.0f}  {h:>14,.0f}  {pct:>+8.3f}%  {verdict}")
        if reason and verdict.startswith("accepted"):
            print(f"{'':<{width}}  {'':>14}  {'':>14}  {'':>9}  reason: {reason}")
    print()
    print(
        f"threshold: a workload fails above +{args.threshold:g}%. Both sides were built and "
        f"measured in this run, on this runner, with this toolchain."
    )

    failed = [r for r in rows if r[4] == "REGRESSION"]

    # The control's own verdict changes what a failure MEANS, so it is stated whenever anything
    # failed rather than left for the reader to notice in the table.
    control = next((r for r in rows if r[0] == CONTROL), None)
    if failed and control is not None:
        axes = [r for r in rows if r[0] != CONTROL]
        if control[3] > args.threshold:
            print(
                f"\nnote: the `{CONTROL}` control moved {control[3]:+.3f}% as well. It reaches no "
                "repetition engine, so a shared cause in the scanner, the lexer or the input "
                "layer fits the evidence better than a change in `parser/many/`."
            )
        elif all(r[3] > args.threshold for r in axes):
            print(
                f"\nnote: every axis moved and the `{CONTROL}` control did not. Whatever changed "
                "is shared by the repetition families and not by the scanner beneath them."
            )

    # A pair that splits says WHERE the cost is paid, which is the reason the pair exists.
    verdicts = {r[0]: r[3] > args.threshold for r in rows}
    for name, failing in sorted(verdicts.items()):
        if not name.endswith(SHALLOW_SUFFIX):
            continue
        deep = name[: -len(SHALLOW_SUFFIX)]
        if deep not in verdicts or failing == verdicts[deep]:
            continue
        if failing:
            print(
                f"\nnote: `{name}` moved past the threshold and `{deep}` did not. They parse the "
                "same elements through the same engine and differ only in how many collections "
                "those elements are spread over, so a cost that reads on the shallow row alone is "
                "paid once per COLLECTION rather than once per element — the shape a consolidation "
                "into shared engines produces, and the shape a deep source amortises away."
            )
        else:
            print(
                f"\nnote: `{deep}` moved past the threshold and `{name}` did not. A per-element "
                "cost reads slightly higher on the deep row, whose per-iteration total is the "
                "smaller of the two; and where the pair is a bounded axis, the deep source is "
                "also the only half whose groups reach their limit, so the `TooMany` "
                "short-circuit is reached there and nowhere else."
            )

    if args.summary:
        with open(args.summary, "a", encoding="utf-8") as fh:
            fh.write("### instruction counts vs merge-base\n\n")
            fh.write("| workload | base Ir/iter | head Ir/iter | delta | verdict |\n")
            fh.write("| --- | ---: | ---: | ---: | --- |\n")
            for name, b, h, pct, verdict, reason, _ in rows:
                note = f" — {reason}" if reason and verdict.startswith("accepted") else ""
                fh.write(f"| `{name}` | {b:,.0f} | {h:,.0f} | {pct:+.3f}% | {verdict}{note} |\n")
            fh.write(
                f"\nThreshold **+{args.threshold:g}%**; both sides built and measured in this "
                "run on this runner.\n"
            )

    if not failed:
        print("icount: no workload regressed.")
        return 0

    print()
    for name, b, h, pct, _, _, _ in failed:
        print(
            f"::error::{name} costs {pct:+.3f}% more instructions than the merge-base: "
            f"{b:,.0f} -> {h:,.0f} Ir per iteration (+{h - b:,.0f}), against a threshold of "
            f"+{args.threshold:g}%."
        )
    print(
        "\nTo see it locally, on a Linux host with valgrind installed:\n"
        "    ci/icount/run.sh <the merge-base sha>\n"
        "\nIf the extra instructions buy something worth having, record the trade in a commit "
        "message on this branch — one trailer per workload:\n"
    )
    for name, _, _, pct, _, _, _ in failed:
        ceiling = max(1.0, round(pct + 1.0))
        print(f"    Perf-accept: {name} +{ceiling:g}% <why the extra instructions are worth it>")
    print(
        "\nThe trailer is read from this branch's own commits, so it expires when they are no "
        "longer this branch's: it licenses one change and does not become a standing allowance "
        "for the workload. `git commit --allow-empty` is enough to add one."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
