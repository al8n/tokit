# Signature-compatible consumers — the shape the generator cannot express

These three consumer bodies reproduce the silent steal the harness **cannot** generate: a
consumer whose method has the same name *and a compatible signature* as tokora's, calling it
with the return value **used**. Each yields, against `main` vs this branch, byte-identical
source and zero diagnostics on both sides:

    base  CONSUMER-CALLS: 1        head  CONSUMER-CALLS: 0

They are kept as source rather than wired into `run.sh` deliberately. Wiring them would
require the generator to synthesise a compatible signature per name, and "how compatible is
compatible enough to be silent" is unbounded — the same enumeration this project has lost
five times. They exist so the finding is reproducible and so a future author does not have
to rediscover that the `loud` verdicts for argument-taking names come from the probe's own
arity (`E0061`), not from the name being safe.

To run one: concatenate `gen_probe.py`'s `FIXTURE`, the consumer body, and its `WITNESS`
into a probe crate's `src/lib.rs`, once against a base checkout of tokora and once against
this tree, and compare the `CONSUMER-CALLS` lines.
