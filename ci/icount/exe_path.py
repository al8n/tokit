#!/usr/bin/env python3
"""Print the `tokora-icount` executable path out of a `cargo build --message-format=json` stream.

Read rather than guessed: `--profile bench` writes its artefacts into `target/release/`, not
`target/bench/`, because cargo's built-in `bench` profile shares release's directory. A path
composed by hand from the profile name is a path that silently names nothing, or — worse —
names a stale `--release` build of the same binary that happens to be sitting there.
"""

import json
import sys

path = None
for line in open(sys.argv[1], encoding="utf-8"):
    line = line.strip()
    if not line or not line.startswith("{"):
        continue
    msg = json.loads(line)
    if (
        msg.get("reason") == "compiler-artifact"
        and msg.get("executable")
        and msg["target"]["name"] == "tokora-icount"
    ):
        path = msg["executable"]
if path is None:
    sys.exit("::error::no `tokora-icount` executable in the build output")
print(path)
