#!/usr/bin/env python3
"""Plant a change the wall-clock gate must NOT catch: prose, and nothing else.

Applied by `ci/wallclock/run.sh --plant docs_only` to the HEAD side of the extracted work tree
only. This is the population a badly built performance gate fails: it recompiles the crate and it
changes what the doc build emits, while changing not one instruction the CPU executes.

It is a slightly harder test than "nothing changed", and deliberately so. Adding lines shifts
every `line!()` and every panic `Location` below them, and those are constants baked into the
binary — same width, different values, so the two sides are not byte-identical objects even
though they are instruction-identical programs. What this plant asks is not that the gate report
zero. A clock never reports zero. It asks that whatever it reports sits inside the floor the
self-comparison established, which is the only sense in which a wall-clock gate can be said not
to fire.

The instruction-count gate ran the same population and reported +0.000% on all thirteen of its
workloads. This one cannot report zero, because a clock never does; what it must report is a
delta inside the floor the self-comparison established. A green here is the statement that the
threshold is above the noise. A red here is the statement that it is not, and that finding is
worth more than the gate.

Two edits, deliberately of the two kinds prose comes in: a `///` line on a public item, and a
`//` line inside the body of the hottest method in the crate.
"""

import pathlib
import sys

TARGET = "tokora/src/input/input_ref/mod.rs"

DOC_ANCHOR = "  #[allow(clippy::should_implement_trait)]\n  pub fn next(\n"
DOC_PLANT = (
    "  /// WALLGATE PLANT (ci/wallclock/plants/docs_only.py): one line of prose on a public\n"
    "  /// item. It changes the rustdoc output and no emitted instruction.\n"
)

BODY_ANCHOR = "    if let Some(cached_token) = self.take_front() {\n"
BODY_PLANT = (
    "      // WALLGATE PLANT (ci/wallclock/plants/docs_only.py): a comment inside the hottest\n"
    "      // line of the hottest method. The compiler discards it before it reaches MIR.\n"
)


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: docs_only.py <extracted-tree>")
    path = pathlib.Path(sys.argv[1]) / TARGET
    text = path.read_text(encoding="utf-8")
    for name, anchor, want in (("doc", DOC_ANCHOR, 1), ("body", BODY_ANCHOR, 2)):
        hits = text.count(anchor)
        if hits != want:
            sys.exit(
                f"::error::{TARGET}: the {name} anchor matched {hits} times, expected {want}. "
                "Re-anchor the plant rather than letting it measure unmodified code."
            )
    # The body anchor occurs twice on purpose — `next` and its sibling — and both get the
    # comment: a plant that has to pick one of two identical lines is a plant that will pick the
    # wrong one the day their order changes.
    text = text.replace(DOC_ANCHOR, DOC_PLANT + DOC_ANCHOR)
    text = text.replace(BODY_ANCHOR, BODY_ANCHOR + BODY_PLANT)
    path.write_text(text, encoding="utf-8")
    print(f"plant: docs_only applied to {TARGET}")


if __name__ == "__main__":
    main()
