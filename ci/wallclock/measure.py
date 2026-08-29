#!/usr/bin/env python3
"""Take one wall-clock reading of every id a criterion bench binary declares.

One invocation is ONE side of ONE round: `run.sh` calls this alternately for the base and head
builds so that the two sides' samples are drawn from the same minutes of the job's life rather
than from its first half and its second.

# The reading is a MINIMUM, not a mean

Criterion collects `sample_size` samples; sample *i* runs some number of iterations and records
the elapsed time for all of them, so `times[i] / iters[i]` is that sample's mean per-iteration
cost. This script reports the SMALLEST of those, and `run.sh` takes the smallest of those across
rounds.

Wall-clock noise is one-directional. A scheduler preemption, a neighbouring container, a page
fault, an interrupt, a frequency drop — every one of them makes a sample slower, and nothing
makes a sample faster than the work it performed. So the mean of a sample set is the true cost
plus an unknown positive noise integral, while the minimum converges to the true cost from
above and is the estimator whose bias shrinks as the noise grows. Criterion's own point estimate
(`slope`, or `mean` under flat sampling) is the right thing to report to a human reading one
run; it is the wrong thing to difference between two builds on a shared runner.

# Why a fresh CRITERION_HOME per invocation

Criterion keeps a `base/` copy of the previous run of the same id and reports a change against
it. That machinery is deliberately unused: it compares this binary against whatever last wrote
that directory, which on a runner is *the other side of this comparison*, and its verdict is a
statistical test at criterion's own confidence level rather than the min-of-N this gate is built
on. A directory nothing else has written cannot contribute a number nobody asked for.

# `--bench` is not optional

Criterion's harness runs in TEST mode — one iteration per id, `Success` printed, no timings and
no output files — unless it is passed `--bench`. `cargo bench` supplies it; a bare invocation of
the binary does not. A test-mode run exits 0 and looks exactly like a fast benchmark run, which
is the silent-green shape this repository has hit six times (#181, #195, #200, #208, #217,
#220). So the flag is passed here and the absence of readings is a hard error below.

Usage:
    measure.py --bin PATH --target NAME --side base|head --round N
               --home DIR --append FILE.jsonl
               [--warm-up-ms MS] [--measurement-ms MS] [--sample-size N] [--nresamples N]
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time


def read_json(path):
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def harvest(home):
    """Map every id criterion just measured to its minimum per-iteration nanoseconds.

    The id is read out of `benchmark.json`'s `full_id` rather than rebuilt from the directory
    path. A criterion group id may contain a slash — `input/scan`, `pratt/typed` — and so may a
    function id — `right_chain/1` — so `<home>/pratt/typed/right_chain/1/new/` cannot be split
    back into (group, id) by counting path components. The binary already wrote down the answer.
    """
    readings = {}
    for dirpath, _dirnames, filenames in os.walk(home):
        if os.path.basename(dirpath) != "new":
            continue
        if "benchmark.json" not in filenames or "sample.json" not in filenames:
            continue
        full_id = read_json(os.path.join(dirpath, "benchmark.json"))["full_id"]
        sample = read_json(os.path.join(dirpath, "sample.json"))
        iters, times = sample["iters"], sample["times"]
        if not iters or len(iters) != len(times):
            sys.exit(f"::error::{dirpath}/sample.json: {len(iters)} iters, {len(times)} times")
        per_iter = [t / i for i, t in zip(iters, times) if i > 0 and t > 0]
        if not per_iter:
            sys.exit(f"::error::{full_id}: every sample recorded zero time or zero iterations")
        if full_id in readings:
            sys.exit(
                f"::error::{full_id} was measured twice in one run — criterion disambiguates "
                "colliding directory names with a numeric suffix, so two ids that differ only "
                "in a character it strips would silently share this reading"
            )
        readings[full_id] = {
            "ns_per_iter": min(per_iter),
            "samples": len(per_iter),
            # The round's own spread, kept because it is what says whether a reading is a
            # measurement or a coin toss. `compare.py` prints the worst of them.
            "sample_spread_pct": (max(per_iter) / min(per_iter) - 1.0) * 100.0,
            "sampling_mode": sample.get("sampling_mode"),
        }
    return readings


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--target", required=True, help="the bench target's name, for the log")
    ap.add_argument("--side", required=True, choices=("base", "head"))
    ap.add_argument("--round", type=int, required=True)
    ap.add_argument("--home", required=True, help="CRITERION_HOME for this invocation")
    ap.add_argument("--append", required=True, help="JSONL file to append this reading to")
    ap.add_argument("--warm-up-ms", type=int, default=200)
    ap.add_argument("--measurement-ms", type=int, default=500)
    ap.add_argument("--sample-size", type=int, default=10)
    ap.add_argument("--nresamples", type=int, default=1000)
    args = ap.parse_args()

    shutil.rmtree(args.home, ignore_errors=True)
    os.makedirs(args.home, exist_ok=True)

    env = dict(os.environ)
    env["CRITERION_HOME"] = args.home
    # The window, through ONE mechanism. `--measurement-time` and `--warm-up-time` are inert in
    # three of the five binaries, which pin the group's values in code; the environment variables
    # are read by `tokora-benches/benches/support/mod.rs` and applied after those pins, so all
    # five answer to the same numbers. See that module for why the command line cannot.
    env["TOKORA_BENCH_WARM_UP_MS"] = str(args.warm_up_ms)
    env["TOKORA_BENCH_MEASUREMENT_MS"] = str(args.measurement_ms)
    env["TOKORA_BENCH_SAMPLE_SIZE"] = str(args.sample_size)

    cmd = [
        args.bin,
        "--bench",
        # No plots and no HTML: they are minutes of CPU across 46 ids and this gate reads JSON.
        "--noplot",
        # Bootstrap resamples feed confidence intervals nothing here reads. The default is
        # 100_000 per id, which at 46 ids is pure overhead on the job's critical path.
        "--nresamples",
        str(args.nresamples),
        "--output-format",
        "bencher",
    ]
    started = time.monotonic()
    proc = subprocess.run(cmd, env=env, capture_output=True, text=True)
    elapsed = time.monotonic() - started
    if proc.returncode != 0:
        sys.exit(
            f"::error::{args.target} ({args.side}, round {args.round}) exited "
            f"{proc.returncode}\n{proc.stdout[-4000:]}\n{proc.stderr[-4000:]}"
        )

    readings = harvest(args.home)
    if not readings:
        sys.exit(
            f"::error::{args.target} ({args.side}, round {args.round}) produced no readings. "
            "A criterion binary that ran in test mode exits 0 and writes nothing; so does one "
            "whose filter matched no id."
        )

    with open(args.append, "a", encoding="utf-8") as fh:
        fh.write(
            json.dumps(
                {
                    "target": args.target,
                    "side": args.side,
                    "round": args.round,
                    "seconds": elapsed,
                    "ids": readings,
                },
                sort_keys=True,
            )
            + "\n"
        )
    print(
        f"wallclock: r{args.round} {args.side:<4} {args.target:<19} "
        f"{len(readings):>2} ids in {elapsed:6.1f}s"
    )


if __name__ == "__main__":
    main()
