#!/usr/bin/env python3
"""Print `name<TAB>path` for every `tokora-benches` bench executable in a cargo JSON stream.

Read rather than composed, for the reason `ci/icount/exe_path.py` gives and one more of its own.
`--profile bench` writes into `target/release/`, not `target/bench/`; and cargo does not put a
bench executable where a reader would guess it does — on the machine this was written, the five
landed under `release/build/tokora-benches/<hash>/out/<name>-<hash>`. A path composed by hand is
a path that names nothing, or names a stale artefact of an older build that happens to be there.

The FLOOR is the other half. `cargo bench --no-run` over a package whose `[[bench]]` section went
missing prints nothing about it and exits 0, and a gate that then measured four targets would
report green over the fifth having disappeared — the shape `bench (smoke)` in `.github/workflows/
ci.yml` already guards with its own `EXPECTED_AT_LEAST`. Raise both when a bench is added.
"""

import json
import sys

EXPECTED_AT_LEAST = 5

found = {}
for line in open(sys.argv[1], encoding="utf-8"):
    line = line.strip()
    if not line or not line.startswith("{"):
        continue
    msg = json.loads(line)
    if msg.get("reason") != "compiler-artifact" or not msg.get("executable"):
        continue
    target = msg["target"]
    if target["kind"] != ["bench"]:
        continue
    if not msg.get("package_id", "").count("tokora-benches"):
        continue
    # A later artefact for the same target supersedes an earlier one; cargo emits one per unit.
    found[target["name"]] = msg["executable"]

if len(found) < EXPECTED_AT_LEAST:
    sys.exit(
        f"::error::the build produced {len(found)} `tokora-benches` bench executables, expected "
        f"at least {EXPECTED_AT_LEAST}: {sorted(found)}. A `[[bench]]` section that goes missing "
        "is not a smaller gate, it is an unmeasured bench."
    )

for name in sorted(found):
    print(f"{name}\t{found[name]}")
