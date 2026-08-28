#!/usr/bin/env python3
"""Take an instruction-count reading of every workload a `tokora-icount` binary declares.

The reading is a DIFFERENCE of two callgrind runs of the same binary at two iteration counts:

    Ir(hi) - Ir(lo)  =  (hi - lo) x per-iteration cost

Nothing inside the binary is toggled, marked or timed. Every fixed cost of the process — the
dynamic loader, `main`'s prologue, building the fixture source, the allocator's first arena,
the process's `envp` copy — appears identically in both runs and cancels exactly, which is
both cheaper and tighter than placing client requests around a measured region and arguing
about what the region includes.

What does NOT cancel is the final `println!`: `sep_while 8 3584` and `sep_while 104 46592`
format different numbers of digits, so a few dozen instructions of the difference are
formatting rather than parsing. That residue is a function of (workload, lo, hi) alone, so it
is IDENTICAL on the two sides of a comparison and cancels exactly there — it is invisible to
the gate. It is visible only to `--linearity`, which reads a third iteration count, and that
check therefore carries a tolerance and prints what it measured.

Usage:
    measure.py --bin <path> --out <file.json> [--iters-lo N] [--iters-hi M] [--linearity]
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

# The two iteration counts. `lo` is not zero and not one: the first iteration through a
# workload warms the allocator's bins with the exact block sizes every later iteration reuses,
# and a count that straddles that transition is not on the line the difference assumes. Both
# are multiples of 8 so that a loop the compiler unrolled runs the same residue at both points.
DEFAULT_LO = 8
DEFAULT_HI = 104

# `--linearity`'s tolerance, as a fraction of the measured difference. The residue it bounds is
# the `println!` formatting described above; anything larger means the per-iteration cost is
# genuinely not constant, which would make the whole differencing model wrong rather than
# slightly imprecise.
LINEARITY_TOLERANCE = 1e-4


def callgrind(binary, args, out_dir, tag):
    """Run `binary args` under callgrind; return (instruction count, stdout)."""
    out_file = os.path.join(out_dir, f"callgrind.{tag}")
    cmd = [
        "valgrind",
        "--tool=callgrind",
        f"--callgrind-out-file={out_file}",
        # Only Ir is wanted. The cache and branch simulators cost wall time and add events
        # this gate does not read; `--collect-systime=no` keeps a clock out of the output.
        "--cache-sim=no",
        "--branch-sim=no",
        "--collect-systime=no",
        binary,
        *args,
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.exit(
            f"::error::callgrind run failed ({' '.join(args)}): exit {proc.returncode}\n"
            f"{proc.stderr}"
        )
    return parse_callgrind(out_file), proc.stdout.strip()


def parse_callgrind(path):
    """Pull the Ir total out of a callgrind output file.

    The `events:` line names the columns; the `summary:`/`totals:` line carries one number per
    column. Reading the column index off `events:` rather than assuming Ir is first is what
    keeps this correct if the run is ever given another event to collect.
    """
    events = None
    totals = []
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            if line.startswith("events:"):
                events = line[len("events:"):].split()
            elif line.startswith(("summary:", "totals:")):
                totals.append([int(n) for n in line.split(":", 1)[1].split()])
    if events is None:
        sys.exit(f"::error::{path}: no `events:` line — callgrind's output format has changed")
    if "Ir" not in events:
        sys.exit(f"::error::{path}: `events: {' '.join(events)}` does not include Ir")
    if not totals:
        sys.exit(f"::error::{path}: no `summary:`/`totals:` line")
    idx = events.index("Ir")
    values = {row[idx] for row in totals}
    if len(values) != 1:
        sys.exit(
            f"::error::{path}: summary and totals disagree on Ir: {sorted(values)} — the "
            "process was not single-threaded, which this reading assumes"
        )
    return values.pop()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--label", default="", help="side name, for the log")
    ap.add_argument("--iters-lo", type=int, default=DEFAULT_LO)
    ap.add_argument("--iters-hi", type=int, default=DEFAULT_HI)
    ap.add_argument(
        "--linearity",
        action="store_true",
        help="take a third reading and require the second difference to vanish",
    )
    args = ap.parse_args()

    if args.iters_hi <= args.iters_lo:
        sys.exit("::error::--iters-hi must exceed --iters-lo")

    names = subprocess.run(
        [args.bin, "--list"], capture_output=True, text=True, check=True
    ).stdout.split()
    if not names:
        sys.exit("::error::the binary declared no workloads")

    span = args.iters_hi - args.iters_lo
    out = {
        "iters_lo": args.iters_lo,
        "iters_hi": args.iters_hi,
        "workloads": {},
    }
    tmp = tempfile.mkdtemp(prefix="icount.")
    try:
        for name in names:
            ir_lo, line_lo = callgrind(args.bin, [name, str(args.iters_lo)], tmp, f"{name}.lo")
            ir_hi, line_hi = callgrind(args.bin, [name, str(args.iters_hi)], tmp, f"{name}.hi")

            # A workload whose checksum does not scale with its iteration count did not run the
            # loop it was asked to run. That is a broken instrument, not a measurement.
            acc_lo = int(line_lo.split()[-1])
            acc_hi = int(line_hi.split()[-1])
            if acc_lo == 0 or acc_hi * args.iters_lo != acc_lo * args.iters_hi:
                sys.exit(
                    f"::error::{name}: checksums {acc_lo} at {args.iters_lo} iterations and "
                    f"{acc_hi} at {args.iters_hi} are not in the ratio of the iteration counts "
                    "— the workload is not doing the same work every iteration"
                )

            delta = ir_hi - ir_lo
            if delta <= 0:
                sys.exit(f"::error::{name}: Ir did not grow with the iteration count ({delta})")
            out["workloads"][name] = {
                "ir_lo": ir_lo,
                "ir_hi": ir_hi,
                "ir_delta": delta,
                "ir_per_iter": delta / span,
                "checksum_per_iter": acc_hi // args.iters_hi,
            }
            print(
                f"icount{args.label}: {name:<16} {delta:>14,} Ir over {span} iterations "
                f"({delta / span:,.1f}/iteration)"
            )

            if args.linearity:
                far = 2 * args.iters_hi - args.iters_lo
                ir_far, _ = callgrind(args.bin, [name, str(far)], tmp, f"{name}.far")
                second = abs((ir_far - ir_hi) - delta)
                limit = max(1, int(delta * LINEARITY_TOLERANCE))
                verdict = "ok" if second <= limit else "FAILED"
                print(
                    f"icount{args.label}: {name:<16} linearity {verdict}: second difference "
                    f"{second} against a tolerance of {limit} "
                    f"({second / delta:.2e} of the reading)"
                )
                if second > limit:
                    sys.exit(
                        f"::error::{name}: per-iteration cost is not constant — Ir at "
                        f"{args.iters_lo}/{args.iters_hi}/{far} iterations is "
                        f"{ir_lo}/{ir_hi}/{ir_far}, whose second difference is {second}. "
                        "The differencing this gate rests on assumes a straight line."
                    )
                out["workloads"][name]["linearity_residue"] = second
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump(out, fh, indent=2, sort_keys=True)
        fh.write("\n")


if __name__ == "__main__":
    main()
