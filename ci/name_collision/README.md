# Name-collision probe harness

Adding a public name is not additive at the source level. A new method, associated
function, free function, type or macro can take over a name a consumer already had, and
whether the consumer is *told* depends on spelling details rustc owns and may change.

Four successive attempts to state a precise rule for this crate were each falsified — never
by a wrong mechanism, always by a **spelling nobody wrote down**. So this directory does
not encode a rule. It runs the experiment, two-sided, over the names a release adds —
subject to the limits below, which are not incidental.

## What it does

`run.sh <base-ref> <inventory.json>` compiles one byte-identical consumer against the base
and against the working tree, **against each name's real owner**, and classifies. Read the
scope limit below before reading a green run as safety:

| verdict | meaning | fatal? |
|---|---|---|
| `loud` | the sides disagree about compiling, or head gains a diagnostic | no — the disclosed outcome |
| `warned` | both ran, and head gained a warning **naming the probed subject** that the base side did not emit | no — rustc reported it |
| `silent*` | a SILENT row listed in `disclosed.txt` | no — known, and in the CHANGELOG |
| `ok*` | both sides agree AND the row is justified in `no_collision.txt` | no |
| `new-owner` | the base side cannot resolve the row's **owner** and the head side can — the same diff introduces the owner as well as the name | no |
| `SILENT` | both compile, neither warns, the witness disagrees, not disclosed | **yes** |
| `UNPROBED` | for an item row, both sides agree and it is **not** justified in `no_collision.txt`; for a glob row, rustc said nothing about the name at all. The glob form cannot be silenced by `no_collision.txt` | **yes** |
| `INCONCL` | no before-state: the base side did not compile, its witness was not 1, or the marker was missing/duplicated | **yes** |
| `FATAL` | the probe could not be generated at all (unknown category, owner, or trait) | **yes** |
| `STALE` | a baselined row **this run probed** that no longer reproduces | **yes** |
| `n/a` | a baselined row this diff gave no occasion to probe — carried, not stale | no |
| `glob-err` | a glob row whose collision the compiler rejected | no — the disclosed outcome |
| `glob-ok` | a glob row this toolchain does **not** reject | no — see below |

**Glob rows are scored separately and deliberately.** A two-glob collision is a
name-resolution outcome — it compiles or it does not — so the generated probe carries no
witness and *cannot* express a silent steal. Running them through the silent/agreement
ladder asked a question they cannot answer: on rustc 1.95 the four macro names' ambiguity is
a **warning**, so both sides compile and the witness agrees. The shipped script scored them
`warned` and passed; once the agreement comparison was fixed they scored `UNPROBED` —
"possibly vacuous probe". Neither is the true answer, which is "this toolchain does not
reject this collision". `glob-ok` is that answer, and it
is why the run header records the toolchain.

Note what is *not* in that list: a bare `ok`. **Both sides agreeing is fatal by default**,
because agreement is the signature of a probe that constructs no collision — the state ten
of the thirteen glob rows sat in while reporting clean. To accept one, justify it by name in
`no_collision.txt`; the gate then prints `ok*` and fails if the justification stops applying.

`disclosed.txt` is the baseline: a silent steal that is known, disclosed and accepted. The
gate fails on a SILENT row that is *not* listed, and **also on a listed row that no longer
reproduces** — an allowlist that can rot into a rubber stamp for a probe that stopped
running is worse than no allowlist. Adding a row there without adding it to the CHANGELOG's
silent section is the thing this is meant to make awkward.

**Staleness is checked only for rows this run planned.** "Did not reproduce" and "was never
attempted" are the same absence and only the first is a finding. The inventory is a *delta*:
a PR is probed for the names it adds, so a row recorded by an earlier release is not in the
plan and cannot reproduce however healthy it is. Checking every row fired on the first
name-adding PR after the release that wrote the baseline — #132 adds one name and saw all
four disclosed rows reported STALE, none of which it touched. A row in the plan and unseen is
STALE and fatal; a row out of the plan prints `n/a` and is carried. That has a cost worth
stating: a baseline row is only reconcilable on a run whose diff adds its name, i.e. the
release that recorded it. Re-probing it on later runs is *not* the fix — by then the name
exists on both sides, the sides agree, and the row scores UNPROBED rather than reproducing.
A two-sided delta cannot re-litigate a name that is no longer new.

**The probe crate is built at the inventory's feature point**, which `run.sh` substitutes
into `probe/Cargo.toml.in`. It used to hardcode `std,logos` while the inventory was derived
at `std,logos,trace,rowan`, so a name behind any other feature was simply not compiled into
the tokora the probe linked against — the probe then collided with nothing and the row
scored `UNPROBED`, *"no collision was constructed"*, when the truth was *"the name was never
there"*. Those read identically in the log, which is the whole reason the two must not be
allowed to differ. Measured: with the feature point forced back to `std,logos`, the
`CastNode` glob row scores `UNPROBED`; at the inventory's own point it scores `glob-err`,
with nothing else changed.

## Mistakes this harness has already made, kept here so it does not make them again

**It probed a dummy type.** The first version exercised every spelling on a local `Gadget`.
That can only collide with a name blanket-implemented for every `Sized` type — true of
exactly one item, the one that was cut for it. `Gadget::parse_except` cannot collide with
`Ident::parse_except`. Probes are now generated per item against the owner the inventory
records, and `gen_probe.py` **exits non-zero on an owner or category it cannot express**
rather than emitting something that compiles and proves nothing.

**CI never fed it the inventory.** The workflow passed only the base ref, so one hard-coded
name ran and 29 did not, and the job was green. The inventory is now a required argument;
a missing file, a parse failure, an incomplete schema, an unknown category and an empty
plan are each fatal. A run over zero names is not a pass.

**The glob probes tested the wrong namespace, the wrong module, and in one case nothing at
all.** `glob_name` created the competing name as a unit struct and referenced it in a *type*
position — but `pinned`, `while_head`, `while_kind`, `dispatch_take` and `try_dispatch_take`
are free functions, which live in the *value* namespace. It also globbed only the crate
root, so it could not reach `tokora::parser::{dispatch_take, try_dispatch_take}` or
`tokora::cst::kinds`. And `glob_macro` declared a local macro and never imported or invoked
tokora's, so all four macro names were untested. **Ten of the thirteen glob rows reported
clean while constructing no collision.** The inventory now records each name's module path
*and* namespace kind, and the generator refuses a kind it has no template for.

**The classifier could read a compile failure as a successful run.** It ignored `cargo`'s
exit status and detected failure by grepping for `^error`; under `CARGO_TERM_COLOR=always`,
which CI sets, ANSI escapes precede the word and the grep matches nothing. It now uses the
exit status, forces `CARGO_TERM_COLOR=never`, and requires **exactly one** `CONSUMER-CALLS`
marker — missing or duplicated is fatal, because a run whose witness cannot be located is
not interpretable. (This is the third time the ANSI-prefix trap has been load-bearing in
this campaign.)

**It hardcoded the trait a probe rides.** `recvr = "try_num" if owner == "TryParseInput" else
"parse_num"` — so a name on any *third* trait got a receiver implementing `ParseInput` and
nothing else, collided with nothing, and would have reported a clean run. The comment two
lines above it said exactly that this must not happen. This one was latent for every future
trait and never fired, because until `CastNode` there was no third trait; it was found by
reading the code rather than by a failing run, which is the only way a false green is ever
found. See *Regenerating the inventory*.

**It could not express a trait item with no receiver, and did not know one from a method.**
The receiver split existed on the inherent side only. See *Categories and spellings*.

**It reconciled baselines it had no occasion to probe.** Every `disclosed.txt` row that the
current PR did not re-probe was reported STALE. See the note under `disclosed.txt` above.

**It probed a different feature point than it inventoried.** See the note above that one.

**It could not express a name whose OWNER the same diff introduces.** #147 added
`RecursionLimitReached` and `NonAssociativeChain` and twelve items on them. `gen_probe.py`
had no template for either owner, so every row was `FATAL` and the job reported an
*incomplete* verdict rather than a finding — which is how it read as a demonstrated
collision when it was nothing of the sort. Adding the templates is only half of it: a probe
that names the owner cannot compile against a base ref where the owner does not exist, and
`no-compile` on the base side was unconditionally `INCONCL`, *"a broken probe, not a clean
result"*. That is the two-causes-one-absence pattern again, and this time the second cause
is not reachable by a better template — a probe is byte-identical on both sides, and a
method call needs a concrete receiver type, so a name on an owner introduced by the same
release has **no consumer call site that could predate it**. The `new-owner` verdict says
that, with the two witnesses listed above. Note what it does *not* say: nothing about a
consumer written after the release. A two-sided delta harness cannot see that, on either
side of the line.

**And it happened again, four owners at once, because the templates are the one part of this
harness that is not derived.** #168 mints `EmitterView` and `Cst`, gives the pre-existing
`ParseState` its first inherent items, and adds `commit_lexer_error` to the pre-existing
`Emitter` trait. Every one of those was `FATAL` — 49 rows of *incomplete verdict*, 14 on `Cst`,
26 on `ParseState`, 6 on `EmitterView` and 3 on `Emitter` — and the
job first said so from CI, because no local gate invokes `run.sh` at all (now written into
`.github/workflows/ci.yml`'s by-hand gate list). Four things came out of writing them, and
each is a rule rather than a one-off:

- **A new owner and a pre-existing owner want OPPOSITE call shapes.** For a new owner the base
  side cannot compile at all, so the only reachable verdict is `new-owner` — and that requires
  the HEAD side to reach `witness=0`, i.e. to compile. Its call must therefore be built at the
  item's real arity and its `used` binding at the item's real return type. For a pre-existing
  owner the BASE side must compile, and there only the consumer's item exists, so the binding
  stays the consumer's `-> u8` and a head-side `E0308` is the honest `loud` verdict. Getting
  this backwards is what put 9 of `RecursionLimitReached`'s 14 rows in `INCONCL`.
- **Which names can reach a new owner's table is bounded, not guessed.** `surface_diff.py`
  gives each name ONE owner: the alphabetically **last** of those declaring it. `Cst` <
  `EmitterView` < `InputRef` < `ParseState`, so the two new owners can only ever receive the
  names they declare *alone* — every forwarder `EmitterView` shares with `InputRef` or
  `ParseState` is routed to those. That is why their tables are complete rather than a subset
  that happens to work today.
- **A subject is sometimes REACHED, never constructed.** `ParseState::new` is `pub(super)` and
  `Cst::from_sink` is `pub(crate)`, so neither can be built directly: the `ParseState` probe
  takes its subject from a real `map_with` callback, and the `Cst` probe runs a real
  `parse_lossless`. The latter drags in a lexer of its own, because `parse_lossless` is walled
  at compile time to tokens declaring `SURFACES_TRIVIA` and the shared fixture's `Tok` is a
  trivia-*skipping* logos lexer — driving `Cst` off it fails post-monomorphization on both
  sides and every row would have read `INCONCL`.
- **A `&mut self` trait method has no expressible competitor here, and that is recorded rather
  than papered over.** `Emitter::commit_lexer_error` takes `&mut self`; the three
  `trait_method` spellings declare `self` (`same_pick`) or `&self` (`later_pick_*`), which on a
  non-deref receiver are both *earlier* picks. The consumer wins on both sides and the rows
  score `UNPROBED` — justified by receiver walk in `no_collision.txt`, the same shape as
  `InputRef::recursion` with the receiver kinds swapped. The spellings are named for a tokora
  method taken **by value**; against a `&mut self` one there is no later pick to name.

All of these are the same defect the harness exists to catch, one level up: **an instrument
that verifies the case you already knew about.** The last four are a sharper version of it —
an instrument that *refuses to answer* looks like a broken gate and gets routed around,
which is why each one is fixed by making the shape probeable rather than by widening what
counts as a pass.

## One verdict ladder, several different questions

Three categories here could not express the question they were being scored on, and each
was found separately as a defect. They are one design fact:

| what is added | what a collision even is | is there a witness? |
|---|---|---|
| an inherent method shadowing a consumer's | one of two bodies runs | **yes** — which body ran |
| an associated function | one of two functions is selected by path | yes in principle, but whether it is *reported* turns on signature proximity |
| a name reachable through a glob | a name-resolution ambiguity — there is no second body to run | **no** — nothing to witness |

The ladder was built for the first row and applied to all three. That is why a glob row
scored `UNPROBED` ("possibly vacuous probe") on a toolchain that simply does not reject the
collision, and why an associated function's `loud` verdict says nothing about the name. The
`glob-err` / `glob-ok` split is not a special case bolted on — it is the first place this
was recognised. **If you add a category, ask what a collision *is* for it and whether
anything can witness one, before deciding which verdicts apply.**

## Every "fine" verdict is asserted by an absence — so each one carries a witness

This harness produced three false-greens in succession, and they were
one defect wearing three faces:

- `silent` asserted because a warning did not fire — in an environment where it *could not* fire;
- agreement asserted because `b != h` was false — after a mutation one line above made it false;
- `glob-ok` asserted because the compile did not fail — when nothing had been compiled.

**An absence is produced equally by the thing not happening and by the measurement not
happening.** Every verdict here that means "fine" is asserted by one, so each now requires
positive evidence that the probe actually constructed what it claims:

| verdict | the witness it must produce |
|---|---|
| `warned` | a head-only diagnostic **naming this probe's subject** — otherwise an unrelated head-side warning would appear on every row and downgrade a genuine `SILENT` |
| `silent*` / `SILENT` | both witnesses parsed and **differing**, *and* control reached the call site on both sides — `witness=0` alone is produced equally by the call being stolen and by the drive path never getting there |
| `glob-err` | a head diagnostic naming the subject **and** mentioning ambiguity — a build that fails for an unrelated reason measured nothing |
| `glob-ok` | an ambiguity diagnostic naming the subject — "both sides compiled" alone cannot distinguish *this toolchain does not reject it* from *no collision was constructed* |
| `ok*` | agreement plus a written justification in `no_collision.txt`, staleness-checked |
| `new-owner` | a base-side unresolved-import / cannot-find / failed-to-resolve diagnostic **naming this row's owner**, *and* the absence of the same diagnostic on head — a template that misspells its owner would otherwise report `new-owner` forever while having compiled nowhere |
| `loud` | the base side ran (`witness=1`), and cargo names **the probe crate** as what failed — so the failure is the probe's, not a dependency's. **Attribution to the collision is still not established**: in the inherent-method and associated-function categories it may be the probe's own arity — see below |

The fatal verdicts — `SILENT`, `UNPROBED`, `INCONCL`, `FATAL`, `STALE` — need no witness, because
they cannot manufacture a passing run.

**A witness that matches text is satisfiable by every producer of that text** — and the
producers include the toolchain, the filesystem and the fixture, not only the code under
test. Twice a witness has been satisfied by the wrong one: the fixture's own naming lint
(`constant \`dispatch_take\` should have an upper case name`) named the subject while having
nothing to do with a collision, and a filesystem path satisfied the word-boundary name test
for a short name — `/opt/homebrew/lib` matches `opt`. The fix in both cases was to remove
the alternative producer rather than to complicate the pattern: require the ambiguity
wording, and drop cargo's own status lines before the warning blocks are built. Confirmed
negatives worth keeping: `try_expect_take_or_stop` does **not** match `try_expect_take`, and
`Option<T>` does **not** match `opt`. Only paths broke it.

**Existence is only half of it; the other half is attribution.** "Is there evidence?" and
"was that evidence produced by the thing under test?" are different questions, and the
second is the one this harness kept failing. Three instances, all now closed: the first
`glob-ok` witness accepted the fixture's own naming lint, which named the subject and had
nothing to do with a collision; `loud` and `glob-err` accepted *any* head-side build
failure, so a broken dependency scored item rows green; and `SILENT` accepted a witness of
zero, which a stolen call and an unreached call site produce identically. When adding a
verdict, enumerate what else could produce its evidence — a dependency, an unrelated lint, a
macro expansion, a diagnostic about the fixture itself — and exclude each, or say the
verdict is unattributed.

Getting the `glob-ok` witness right took two attempts, and the first failure is worth
keeping: requiring merely that rustc *named* the subject passed on a vacuous probe, because
the fixture's own naming lint says "constant `dispatch_take` should have an upper case
name" — the name, in a diagnostic with nothing to do with a collision. A witness has to be
evidence of the thing, not of a word.

## What this harness does NOT test — read this before trusting a green run

**For the three item categories it probes exactly one shape of consumer: an extension item
whose signature is *incompatible* with tokora's** — `-> u8`, hardcoded in **all three** item templates:
`inherent_method` (`fn name(&mut self) -> u8`), `inherent_assoc_fn`
(`fn name() -> u8`) and `trait_method`. That shape produces the *loud* outcome, and the
harness reports it faithfully.

**It cannot generate the shape that matters most: a consumer whose signature is
compatible** — someone who wrote the helper themselves before tokora had it. For those,
tokora's method takes the call silently **even when the return value is used**, which is
the opposite of what the incompatible-signature probe reports. Measured for `peek_kind`,
`head_satisfies` and `peek_head_map`: byte-identical consumer, zero diagnostics on both
sides, `CONSUMER-CALLS: 1` on base and `0` on head.

**Why there is no list here of "the names that are safe".** Neither this harness nor any
document in this branch can produce one. Whether a collision is reported depends on how
close a consumer's signature is to tokora's, and that is a property of code that does not
exist yet — so the count of `loud` rows is a count of *what was probed*, never a count of
names that are fine.

**The associated-function category has the identical gap**, and it is easy to miss because
the two shapes look different. `inherent_assoc_fn` declares `fn name() -> u8` — zero
parameters — while `Ident::parse_except` and `Ident::try_parse_except` take two. So their
`loud` verdicts are arity verdicts about the probe, exactly like the methods', and nothing
in this harness establishes that a path-resolved collision must be loud.

**Read that as unsupported, not disproven.** Two attempts to construct a silently-stealing
consumer for `parse_except` produced a compile error; a different reviewer, using a
different signature, produced a silent steal. Neither settles the category, because whether
a collision is reported depends on how close the consumer's signature is to tokora's — a
property of code that does not exist yet. That is also why there is no list here of "the
names that go silent": such a list is not a wrong answer, it is not a well-formed one.

Consequently, for the **inherent-method and associated-function** categories, an added name
that takes arguments makes the head side fail with `E0061` — the probe's own arity, not the
collision — and the row is filed `loud`. That verdict is honest about what was run and
**misleading about the name**: it means "this probe's consumer collides loudly", not "this
name cannot be stolen silently". (The trait-method templates *do* pass real arguments, so
their rows are the harness's strongest: `labelled/later_pick_used` fails with `E0308`, a
type verdict, and `labelled/later_pick_discarded` is a genuine `silent*`.)

Two things follow, and neither is a bug to be fixed by another round of generator work:

1. **A green run means: no silent steal *of the shape this harness can express*.** It is
   not a proof of absence. The CHANGELOG says so in the same terms, and says it there
   rather than only here, because a consumer reads the CHANGELOG.
2. **Generalising to arbitrary consumer signatures is unbounded.** "How close is close
   enough to be silent" depends on the consumer's own generics, receiver and return type;
   enumerating that is the same losing game as enumerating resolution rules, which this
   project has now lost five times. The harness stays at the shape it can express, and the
   gap is disclosed instead of papered over.

**It does not test its own `TRAITS` records — they are trusted, not verified.** A trait row
names the receiver its probe rides (`recvr` in `gen_probe.py`), and no run can check that the
named value implements the trait: a subject that does *not* constructs no collision and reports
a clean run, which is byte-identical to the clean run a correct subject produces whenever the
consumer's item wins at an earlier pick. Measured, for the worst shape — a `&mut self` trait
method on a non-deref subject, where the consumer's blanket item claims every pick and tokora's
is never a candidate: swapping in a receiver that implements nothing produced the identical
`7u8` from the consumer's item and the identical `CONSUMER-CALLS` witness on both sides — a
byte-identical `ok*`, with no column in which the two differ. Such a row **cannot be falsified
by running it**, so its `no_collision.txt` justification, and the reviewer who agrees with it, are
the whole of its protection. That is a property of method resolution rather than a gap in the
templates, which is why it appears here as a bound and not on a fix list. Approving a record is
therefore a **review act, not a mechanical one**: read the pick analysis, not the verdict
column, and never take a green `ok*` on such a row as evidence that its subject was right.

## Categories and spellings

| category | spellings | why |
|---|---|---|
| `inherent_method` | `used`, `discarded` | a discarded return removes the type disagreement that makes it loud |
| `inherent_assoc_fn` | `used`, `discarded` | path resolution, not the receiver walk — a different rule |
| `trait_method` | `same_pick`, `later_pick_used`, `later_pick_discarded` | `self` collides at the same pick; `&self` sits at a later one, which is where the silent class lives |
| `trait_assoc_fn` | `used`, `discarded` | a trait item that declares **no receiver**. Same reason as `inherent_assoc_fn` and the same spellings: there is no receiver chain to walk, so the `*_pick` spellings name nothing |
| `trait_assoc_item` | `no_template` | an associated **type or const** on a trait. There is no probe; the row exists so the refusal is attached to the item instead of the item being dropped |
| `glob_name` / `glob_macro` | `clash` | two competing globs; item names and macro names differ, and macro names differ by toolchain |

**The receiver split applies to trait items too, and did not.** `surface_diff.py` split the
*inherent* side into receiver methods and associated functions — they resolve by different
rules — and put every trait item in one bucket. So a receiver-less trait associated function
was indistinguishable from a `&self` method, and `gen_probe.trait_method` hardwires
`{recvr}.{name}(..)`, which cannot typecheck for an item that takes no receiver.
`CastNode::cast_node` is the shape that found it: three FATAL rows, and a genuinely new
public name left unguarded. The trait side now carries the same two facts, plus *is this a
function at all* — an associated type used to land in the method bucket and fail with a
message about a missing argument template, which named the wrong problem.

The trait-assoc-fn probe is a **path** collision: the consumer's own `impl<T> ConsumerAssoc
for T` and tokora's trait are both applicable to a type that satisfies tokora's bound, and
rustc refuses to choose (E0034). Its honest verdict is therefore `loud`, never `silent` —
path resolution walks no autoderef chain, so there is no later pick to lose the call at
quietly. A `silent` row in this category would be news.

A warning counts as a diagnostic. `#[must_use]` on a discarded return is precisely the
breadcrumb that separates *silent* from *quiet but reported*, and conflating them would
overstate the hazard where the crate relies on it.

## Regenerating the inventory

    python3 ci/name_collision/surface_diff.py base.json head.json --features "std,logos,trace,rowan"

from rustdoc-JSON dumps of both sides. It splits **receiver methods** from **associated
functions** on *both* the inherent and the trait side — they resolve by different rules —
and records each name's owner, because a probe that does not know the owner cannot construct
the collision. The `--features` string is echoed into the inventory and `run.sh` builds the
probe crate at exactly that point; regenerating at one feature point and probing at another
is a silent disagreement about which surface is under test.

For a trait item the owner is the **trait**, and `gen_probe.py` looks it up in its `TRAITS`
table rather than defaulting. It used to default:

```python
recvr = "try_num" if owner == "TryParseInput" else "parse_num"
```

so every trait that was not `TryParseInput` rode a receiver implementing `ParseInput` and
nothing else. That is worse than an unexpressible shape: a probe whose subject does not
implement the trait constructs no collision and reports a clean run over an experiment that
never happened. An unknown trait, or a trait with no entry for the field the row needs, is
now FATAL and names what to add.
