#!/bin/bash
#
# Two-sided name-collision probe, driven by the GENERATED surface inventory.
#
#   run.sh <base-git-ref> <surface_names.json>
#
# For the names a release adds, compile one byte-identical consumer against the base ref
# and against the working tree, against the name's REAL owner, and classify:
#
#   loud      the sides disagree about compiling, or head gains a diagnostic   non-fatal
#   warned    both ran and head gains a warning NAMING the probed subject       non-fatal
#   silent*   a SILENT row listed in disclosed.txt                              non-fatal
#   ok*       both sides AGREE and the row is justified in no_collision.txt     non-fatal
#   new-owner base cannot resolve the OWNER and head can — the same diff        non-fatal
#             introduces the owner, so no call site could predate the name
#   new-owner-err  the same, where head REJECTS the collision (E0034) instead    non-fatal
#             of taking it — the loud outcome, reachable on one side only
#   new-owner-nc*  the same, where the CONSUMER's item wins an earlier pick and  non-fatal
#             no collision is constructed; justified by pick order in no_collision.txt
#   SILENT    both ran, neither warned, witness differs, not disclosed          FATAL
#   UNPROBED  both sides agree and it is NOT justified — likely a vacuous probe FATAL
#   INCONCL   no before-state (base did not run, or the witness was unreadable) FATAL
#   FATAL     the probe could not be generated at all                           FATAL
#   STALE     a baselined row THIS RUN PROBED that no longer reproduces         FATAL
#   n/a       a baselined row this diff gave no occasion to probe               non-fatal
#   glob-err  a glob row whose collision the compiler rejected                  non-fatal
#   glob-ok   a glob row this toolchain does NOT reject                         non-fatal
#
# The probe crate is built at the INVENTORY's feature point, not at a point written here. A
# name gated behind a feature the probe does not enable is absent from the tokora it links
# against, and its row then scores UNPROBED — indistinguishable in the log from "no collision
# was constructed", which is a different finding entirely.
#
# For the ITEM categories there is deliberately no bare `ok`: agreement is the signature
# of a probe that constructs no collision, so it is fatal unless justified by name.
#
# Glob rows are scored separately and do NOT carry that check. A glob collision has no
# witness, so `glob-ok` cannot distinguish "this toolchain does not reject it" from "this
# probe constructs no collision" — see README.md.
#
# `loud` is the disclosed, acceptable outcome. `SILENT` is what no diagnostic can report
# and is what this harness exists for.
#
# Everything here fails loudly rather than skipping. A missing inventory, a JSON parse
# failure, a category with no template, or an owner with no template is FATAL — a harness
# that silently probes zero names is the exact defect it was built to catch, and the first
# version of this script had it.

set -uo pipefail

BASE_REF="${1:?usage: run.sh <base-git-ref> <surface_names.json>}"
INVENTORY="${2:?FATAL: the inventory is REQUIRED — a probe run over no names is not a pass}"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

[ -f "$INVENTORY" ] || { echo "name-collision: FATAL — inventory $INVENTORY does not exist" >&2; exit 2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tokora-name-collision.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# The base tree. `git archive` omits everything .gitignore lists — Cargo.lock included — so
# the lock is copied across explicitly. Without it the two TREES can resolve DIFFERENT
# dependency versions and the comparison stops being about this crate at all. Scope: this
# pins the extracted base tree only. The generated probe crates are separate packages with
# no lock of their own and are not covered here — see the probe-side copy further down.
mkdir -p "$WORK/base"
# `git archive | tar -x` is a pipe under `set -o pipefail` (:49). `tar -x` can stop reading
# once it has what it wants; `git archive` then takes SIGPIPE on the now-closed pipe and exits
# 141, and pipefail reports the rightmost NON-ZERO exit in the pipeline — 141, from git, not
# tar's own (successful) status. That FATAL is a race against archive size and pipe buffering,
# not a real extraction failure: `|| [ $? -eq 141 ]` or `trap '' PIPE` would suppress it, but
# both make a GENUINE extraction failure indistinguishable from a benign SIGPIPE. Break the
# pipe instead — archive to a file, then extract from the file, so nothing is reading the
# other end while git is still writing it.
git -C "$ROOT" archive --format=tar -o "$WORK/base.tar" "$BASE_REF" || {
  echo "name-collision: FATAL — could not archive base ref $BASE_REF" >&2; exit 2; }
tar -x -f "$WORK/base.tar" -C "$WORK/base" || {
  echo "name-collision: FATAL — could not extract base ref $BASE_REF" >&2; exit 2; }
# Check the RESULT, not the status: a tar that exits 0 said only that it processed its input
# without error, not that the tree it produced is the crate this script is about to build
# against ($WORK/base/tokora, below). Cargo.toml at the extracted root is the cheapest positive
# evidence that the archive was not empty, truncated, or extracted to the wrong place.
[ -f "$WORK/base/Cargo.toml" ] || {
  echo "name-collision: FATAL — extraction of base ref $BASE_REF produced no Cargo.toml under" >&2
  echo "name-collision: $WORK/base — the archive was empty, truncated, or misplaced" >&2
  exit 2; }
[ -f "$ROOT/Cargo.lock" ] && cp "$ROOT/Cargo.lock" "$WORK/base/Cargo.lock"

# Flatten the inventory into `category<TAB>name<TAB>owner<TAB>spelling` rows. A parse
# failure is fatal; so is an empty plan.
#
# Second output, second argument: the FEATURE POINT the probe crate must build tokora at.
# The inventory is a fact about one feature point and records which; the probe crate used to
# hardcode `std,logos` regardless. Every name behind any other feature was then absent from
# the probe's tokora, so its probe collided with nothing and scored UNPROBED — which is how
# `CastNode`, gated on `rowan`, came back "no collision was constructed" when the truth was
# "the name was not compiled in". Deriving it from the inventory keeps the two sides of the
# harness looking at the same surface by construction.
python3 - "$INVENTORY" "$WORK/features.txt" > "$WORK/plan.tsv" <<'PY' || { echo "name-collision: FATAL — inventory parse failed" >&2; exit 2; }
import json, sys
d = json.load(open(sys.argv[1]))
required = ("inherent_method_names", "inherent_assoc_fn_names", "trait_method_names",
            "trait_assoc_fn_names", "trait_other_names",
            "glob_names", "glob_details", "inherent_owners", "trait_method_owners",
            "feature_point")
missing = [k for k in required if k not in d]
if missing:
    sys.exit("inventory is missing %s — regenerate it with surface_diff.py" % missing)
feats = [f.strip() for f in d["feature_point"].split(",") if f.strip()]
# The shared fixture is written against `std` and `logos` — it declares a `Logos` token type
# and drives a `LogosLexer`. A feature point without them does not describe a surface this
# fixture can probe at all, and building it anyway would fail on both sides and report
# INCONCL over every row, which says nothing about the names.
lack = [f for f in ("std", "logos") if f not in feats]
if lack:
    sys.exit("inventory feature point %r lacks %s, which the probe fixture is written "
             "against — regenerate the inventory at a feature point that includes them"
             % (d["feature_point"], lack))
with open(sys.argv[2], "w") as fh:
    fh.write(", ".join('"%s"' % f for f in feats))
io, to = d["inherent_owners"], d["trait_method_owners"]
rows = []
for n in d["inherent_method_names"]:
    for sp in ("used", "discarded"):
        rows.append(("inherent_method", n, io[n], sp))
for n in d["inherent_assoc_fn_names"]:
    for sp in ("used", "discarded"):
        rows.append(("inherent_assoc_fn", n, io[n], sp))
for n in d["trait_method_names"]:
    for sp in ("same_pick", "later_pick_used", "later_pick_discarded"):
        rows.append(("trait_method", n, to[n], sp))
# A receiver-less trait associated function takes the INHERENT associated-function
# spellings, not the method ones: `same_pick`/`later_pick_*` name steps of the receiver's
# autoderef walk, and a path call has no receiver to walk.
for n in d["trait_assoc_fn_names"]:
    for sp in ("used", "discarded"):
        rows.append(("trait_assoc_fn", n, to[n], sp))
# An associated TYPE or CONST on a trait — two different resolution classes, and only one of
# them is expressible here. An associated CONST is reached by a path, so a consumer's own const
# of the same name on the same type is a second applicable candidate and rustc reports E0034:
# that is a real collision and it gets a real probe. An associated TYPE resolves in the type
# namespace by rules `gen_probe.py` still has no template for, so it keeps the refusal — which
# exits non-zero by name, so the row is attached to the item rather than dropped from the plan.
# Dropping it is the failure mode this whole harness was built after.
#
# An absent or unrecognised kind routes to the REFUSAL, never to the probe: a probe riding a
# guessed resolution class is the vacuous experiment this harness exists to refuse.
tok = d.get("trait_other_kinds", {})
for n in d["trait_other_names"]:
    if tok.get(n) == "assoc_const":
        for sp in ("used", "discarded"):
            rows.append(("trait_assoc_const", n, to[n], sp))
    else:
        rows.append(("trait_assoc_item", n, to[n], "no_template"))
# Presence, not truthiness. `{}` is what a well-formed inventory looks like when the diff
# added no glob-reachable names; treating that as a malformed inventory turns "this PR adds
# no public surface" into a FATAL. Absence is still fatal, and the required-key check above
# is where that is caught.
gd = d["glob_details"]
for n in d["glob_names"]:
    det = gd.get(n)
    if not det:
        sys.exit("glob name %r has no details entry — refusing to probe it blind" % n)
    cat = "glob_macro" if det["kind"] == "macro" else "glob_name"
    rows.append((cat, n, "%s:%s" % (det["path"], det["kind"]), "clash"))
if not rows:
    # Zero probes has two causes and they are opposite. If every name list is empty the diff
    # genuinely added no public names, there is no collision to construct, and an empty plan is
    # the correct output. If any list is non-empty then rows should have been built from it and
    # something is wrong with this flattening — still fatal.
    named = (d["glob_names"] or d["trait_method_names"] or d["trait_assoc_fn_names"]
             or d["trait_other_names"]
             or d["inherent_method_names"] or d["inherent_assoc_fn_names"])
    if named:
        sys.exit("inventory lists names but produced ZERO probes — that is a failure, not a pass")
for r in rows:
    print("\t".join(r))
PY

total=$(wc -l < "$WORK/plan.tsv" | tr -d ' ')
FEATURES="$(cat "$WORK/features.txt")"
[ -n "$FEATURES" ] || { echo "name-collision: FATAL — no feature point derived from the inventory" >&2; exit 2; }
echo "name-collision: base=$BASE_REF  head=<working tree>"
echo "name-collision: inventory=$INVENTORY  probes=$total"
echo "name-collision: tokora features=[$FEATURES]  (from the inventory's feature point)"
echo "name-collision: rustc=$(rustc --version 2>/dev/null || echo unknown)"

# The labels this run had an OCCASION to construct — `name/spelling`, one per planned row.
# The baseline reconciliation at the bottom is scoped to these; see the comment there.
awk -F'\t' 'NF >= 4 { print $2 "/" $4 }' "$WORK/plan.tsv" | sort -u > "$WORK/planned.txt"

DISCLOSED="$HERE/disclosed.txt"
NOCOLLIDE="$HERE/no_collision.txt"
# Checked before the empty-plan return, not after. A deleted baseline file is a finding on
# every run, including the runs that have nothing to probe — otherwise a no-op PR is the one
# place the harness would accept a missing baseline.
for f in "$DISCLOSED" "$NOCOLLIDE"; do
  [ -f "$f" ] || { echo "name-collision: FATAL — $f is missing" >&2; exit 2; }
done

# An empty plan from a well-formed inventory means the diff added no public names at all. The
# python above is fail-closed, so reaching here with zero rows IS that case rather than a parse
# problem. Return before the probe loop: there is nothing to compile. Reconciliation would now
# be a no-op anyway — with an empty plan every baseline row is out of scope — but saying it
# here keeps the empty case readable without making the reader derive it.
if [ "$total" -eq 0 ]; then
  echo "name-collision: PASS — the diff adds no new public names, so there is no name for a"
  echo "name-collision: consumer to collide with and nothing for this harness to construct."
  echo "name-collision: The baselines in disclosed.txt and no_collision.txt are NOT reconciled"
  echo "name-collision: on this run: with zero probes they cannot reproduce, and reporting them"
  echo "name-collision: stale would be an artifact of an empty diff rather than a finding."
  exit 0
fi

seen_disclosed=""
seen_nocollide=""

status=0
silent=0
newsilent=0
unprobed=0
fatal=0
globerr=0
globok=0
loud=0
okc=0
incon=0
newowner=0
newownererr=0
newownernc=0
while IFS=$(printf '\t') read -r cat name owner spelling; do
  [ -n "$cat" ] || continue
  if ! python3 "$HERE/gen_probe.py" "$cat" "$name" "$owner" "$spelling" > "$WORK/probe.rs" 2> "$WORK/gen.err"; then
    echo "  FATAL   $name ($cat/$spelling): $(cat "$WORK/gen.err")" >&2
    fatal=$((fatal + 1))
    status=2
    continue
  fi
  b=""; h=""
  : > "$WORK/warn-base.txt"; : > "$WORK/warn-head.txt"
  for side in base head; do
    dep="$ROOT/tokora"; [ "$side" = base ] && dep="$WORK/base/tokora"
    d="$WORK/probe-$side"; mkdir -p "$d/src" "$d/otherlib/src"
    # The competing crate for a macro clash. `#[macro_export]` lands at a crate root, so a
    # second exporter cannot live in the probe itself.
    python3 "$HERE/gen_probe.py" --otherlib "$name" > "$d/otherlib/src/lib.rs"
    printf '[package]\nname = "otherlib"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n' > "$d/otherlib/Cargo.toml"
    sed -e "s|TOKORA_PATH|$dep|" -e "s|TOKORA_FEATURES|$FEATURES|" \
      "$HERE/probe/Cargo.toml.in" > "$d/Cargo.toml"
    # A placeholder that survives substitution is not a manifest cargo can read, and the
    # failure would surface as `upstream-fail` on every row — a whole run of INCONCL blamed
    # on a dependency. Say what it actually is, once, here.
    if grep -q TOKORA_ "$d/Cargo.toml"; then
      echo "name-collision: FATAL — an unsubstituted placeholder is left in $d/Cargo.toml" >&2
      exit 2
    fi
    cp "$WORK/probe.rs" "$d/src/lib.rs"
    # The lock copied into `$WORK/base` above pins the base TREE. It does not reach here:
    # these are freshly generated crates with no lock of their own, so each side would
    # resolve independently and a version skew between them would show up as a build
    # difference attributed to the added names. Base resolves first; head is handed base's
    # resolution so the two agree. Deliberately without `--locked`: if a future wave adds a
    # dependency the sides genuinely differ, and re-resolving the changed part is better
    # than a hard failure in a harness whose job is to report on something else.
    if [ "$side" = head ] && [ -f "$WORK/probe-base/Cargo.lock" ]; then
      cp "$WORK/probe-base/Cargo.lock" "$d/Cargo.lock"
    fi
    # `CARGO_TERM_COLOR=never`: with colour on, cargo prefixes diagnostics with ANSI
    # escapes, so `^error` matches nothing and a compile failure reads as a successful run
    # with no witness. That is the third time this trap has been load-bearing in this
    # campaign, and here it sat inside the classifier itself.
    # RUSTFLAGS is cleared deliberately, and it cuts both ways — say so here rather than
    # let it be rediscovered. Clearing it is what makes `warned` decidable: the whole
    # silent/reported distinction is a warning firing, and CI's `-Dwarnings` promotes every
    # warning to an error, collapsing that distinction into `no-compile`. The same act also
    # means a probe build can no longer fail the way a lint-strict consumer's build would.
    #
    # That is acceptable for one reason, and only this one: **the probe fixture is not
    # consumer code.** It is an instrument whose lints (naming, unused imports) are
    # artefacts of generation, and it asserts nothing about whether tokora itself is
    # warning-clean — `cargo clippy --all-features -D warnings` on the crate does that, in
    # its own job. If this harness ever grows a check that depends on a probe FAILING to
    # build under a consumer's lint settings, this line is what makes that check
    # unobservable, and it must be revisited rather than worked around.
    out=$(cd "$d" && CARGO_TERM_COLOR=never RUSTFLAGS= RUSTDOCFLAGS= CARGO_ENCODED_RUSTFLAGS= \
          CARGO_TARGET_DIR="$WORK/tgt-$side" cargo test -- --nocapture 2>&1)
    rc=$?
    printf '%s\n' "$out" > "$WORK/out-$side.txt"
    # The EXIT STATUS is the authority on whether the build and run succeeded; grepping is
    # only used to classify a run that actually happened.
    if [ "$rc" -ne 0 ]; then
      # WHICH crate failed? A non-zero exit says only that something did. If `tokora` or
      # `otherlib` failed to build, the probe was never compiled and measured nothing — yet
      # `loud` and `glob-err` both read a head-side failure as "rustc rejected the
      # collision". Requiring cargo to name the PROBE crate is what makes a failure
      # attributable; without it a syntax error anywhere upstream scores every item row
      # green.
      if grep -q "could not compile \`name-collision-probe\`" "$WORK/out-$side.txt" 2>/dev/null; then
        r="no-compile"
      else
        r="upstream-fail"
      fi
    else
      # Exactly one marker. Missing means the drive path never reached the call; more than
      # one means the probe ran something other than what it claims. Either way the result
      # is not interpretable, and accepting it is how a vacuous probe reads as evidence.
      # BOTH markers get the exactly-one rule, and for the same reason. `REACHED` used to be
      # read with `head -1` over the whole cargo stream, which made the guard below
      # defeatable: a witness that matches text is satisfiable by every producer of that
      # text, and cargo's stream carries more producers than the probe — a dependency build
      # script's output, or a compiler note quoting the probe's own source line, supplies an
      # earlier `REACHED: 1` and the real `REACHED: 0` is never read. The guard then passes a
      # row that never entered its call site, which is precisely what the guard exists to
      # stop. Counting is what makes the marker attributable to the one `println!` that is
      # supposed to emit it (`gen_probe.py`); a second occurrence means something else
      # produced it, and that is not interpretable either.
      # Occurrences, not matching lines: `grep -c` counts lines, so two markers printed on
      # one line would read as one and the extraction below would then yield two numbers
      # into a scalar comparison.
      nmark=$(printf '%s\n' "$out" | grep -oE 'CONSUMER-CALLS: [0-9]+' | wc -l | tr -d ' ')
      nrch=$(printf '%s\n' "$out" | grep -oE 'REACHED: [0-9]+' | wc -l | tr -d ' ')
      if [ "$nmark" -ne 1 ] || [ "$nrch" -ne 1 ]; then
        r="bad-witness(calls=$nmark,reached=$nrch)"
      else
        w=$(printf '%s\n' "$out" | grep -oE 'CONSUMER-CALLS: [0-9]+' | grep -oE '[0-9]+$')
        rch=$(printf '%s\n' "$out" | grep -oE 'REACHED: [0-9]+' | grep -oE '[0-9]+$')
        # Attribution, not existence. `witness=0` is produced equally by the added name
        # taking the call and by control never reaching the call site — both are "the
        # consumer's item did not run". A row whose call site was not entered measured
        # nothing, whichever way its witness reads.
        if [ "${rch:-0}" -eq 0 ]; then
          r="unreached"
        else
          r="witness=${w}"
        fi
        # Cargo's own status lines are dropped BEFORE the blocks are built. The final
        # block otherwise absorbs `Running unittests <path>`, and the name witness is a
        # word-boundary match — so a short probed name matches a PATH fragment. `/opt/`
        # satisfies the witness for `opt`. That is the absence class again: evidence that
        # exists, matches, and is produced by the filesystem layout rather than by a
        # collision. Names at risk are the short ones; the prefix case is already safe
        # (`try_expect_take` does not match inside `try_expect_take_or_stop`).
        # The warning TEXTS, kept for a set difference below. A warning the probe itself
        # provokes appears on BOTH sides and is not a breadcrumb the change produced;
        # treating it as one downgrades a silent steal to `loud` and the gate passes.
        printf '%s\n' "$out" \
          | grep -vE '^ *(Compiling|Fresh|Finished|Running|Blocking|Downloaded|Updating) ' \
          | awk '/^warning(:|\[)/ {blk=$0; inblk=1; next}
                 inblk && /^[[:space:]]*$/ {print blk; inblk=0; next}
                 inblk {blk=blk " | " $0}
                 END {if (inblk) print blk}' \
          | sort -u > "$WORK/warn-$side.txt"
      fi
    fi
    [ "$side" = base ] && b="$r" || h="$r"
  done
  # Only a warning the head side GAINS counts. `comm -13` = present in head, absent in base.
  # `h_raw` keeps the UNMUTATED outcome: the agreement test below must compare outcomes, not
  # outcomes-plus-annotation, or a head-only warning silently routes an agreeing row past
  # the fatal branch. Four glob rows did exactly that on the stable toolchain.
  h_raw="$h"
  # The head-only warning must NAME this probe's subject. Without that test any unrelated
  # head-side warning — a new deprecation in tokora, say — would appear on every row and
  # downgrade a genuine SILENT to the non-fatal `warned`, which is a false green produced
  # by a diagnostic about something else entirely.
  #
  # `comm` is captured before it is matched, not piped into `grep -q`: under pipefail, a
  # `grep -q` that matches early closes the pipe while `comm` may still be writing, `comm`
  # then takes SIGPIPE and exits 141, and pipefail reports THAT instead of grep's true 0 — the
  # `if` would read "no warning" for a row that has one. Capturing once and matching the
  # capture keeps the semantics exact with no live pipe left to race.
  new_warnings="$(comm -13 "$WORK/warn-base.txt" "$WORK/warn-head.txt" 2>/dev/null)"
  if [ -s "$WORK/warn-head.txt" ] \
     && grep -qE "(^|[^A-Za-z0-9_])$name([^A-Za-z0-9_]|$)" <<<"$new_warnings"; then
    case "$h" in witness=*) h="${h},warned" ;; esac
  fi

  label="$name/$spelling"

  # Glob rows are not steals and never were. A two-glob collision is a name-resolution
  # outcome — it compiles or it does not — and the generated probe's `drive()` is an
  # unconditional `ran()`, so its witness is a constant. Running them through the
  # silent/agreement ladder asked a question they cannot answer: on 1.95 the four macro
  # names' ambiguity is a *warning*, both sides compile, the witness agrees, and they were
  # scored `UNPROBED` — "possibly vacuous probe" — when the real answer is "this toolchain
  # does not reject this collision". They get their own two verdicts, and the toolchain is
  # in the run header because that is what decides which one.
  case "$cat" in
    glob_name|glob_macro)
      # Both of these verdicts used to be asserted by an ABSENCE — "the head did not
      # compile" and "the head did compile" — and an absence is produced equally by the
      # thing not happening and by the measurement not happening. Each now requires rustc
      # to have SAID something naming this probe's subject. That is the zero-grep rule
      # applied to a verdict: a clean compile is not evidence of non-rejection until the
      # probe is shown to construct the collision at all.
      # The witness must be evidence of the AMBIGUITY, not merely a mention of the name.
      # A first attempt tested only for the name and passed on a vacuous probe, because the
      # fixture's own naming lint says "constant `dispatch_take` should have an upper case
      # name" — the name, in a diagnostic that has nothing to do with a collision. Requiring
      # the ambiguity wording is what makes this a witness rather than a proxy for one.
      # Captured once, matched against the capture — not piped into `grep -q`. `grep -qiE`
      # exits as soon as it matches; under pipefail the writing `grep` can then take SIGPIPE
      # on the still-open pipe and its 141 outranks the reader's true 0, so `said` would stay
      # unset for a row that DID say "ambiguous". Same fix as `new_warnings` above.
      said=""
      name_lines="$(grep -E "(^|[^A-Za-z0-9_])$name([^A-Za-z0-9_]|$)" "$WORK/out-head.txt" 2>/dev/null)"
      grep -qiE "ambiguous|E0659" <<<"$name_lines" && said=yes
      # The base side must be one of the two shapes a glob row can actually produce on an
      # ordinary run before head's evidence means anything: `no-compile` (rustc rejected the
      # probe outright — its own INCONCL below) or `unreached` (it compiled — `glob_name` and
      # `glob_macro`'s `drive()` calls only `ran()`, never `reached()`, so a successful compile
      # classifies as `unreached`, NEVER `witness=*`, on either side). `upstream-fail`,
      # `bad-witness(...)`, and a `witness=*` these templates cannot produce all mean the base
      # side never reached a normal, comparable conclusion — and nothing here was checking for
      # them: this branch used to match the LITERAL STRING `no-compile` and nothing else, so any
      # OTHER broken base classification fell straight through into the head-only checks below
      # and let a head-side ambiguity alone decide `glob-err`/`glob-ok` with no valid
      # before-state at all. An ALLOWLIST of the shapes that are actually evidence, not a
      # denylist of the shapes already known to be broken — a denylist is silent about the next
      # broken shape nobody has named yet, which is exactly how this one went unnoticed (see the
      # `new-owner` verdict below for the same rule, stated the first time).
      case "$b" in
        no-compile | unreached) ;;
        *)
          printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
          echo "          the base side did not reach a normal compile-or-reject conclusion"
          echo "          (base=$b), so there is no valid before-state for this glob row — a"
          echo "          head-side ambiguity alone cannot stand in for one"
          incon=$((incon + 1)); status=2
          continue
          ;;
      esac
      if [ "$b" = no-compile ]; then
        printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
        echo "          the base side did not compile, so nothing was compared"
        incon=$((incon + 1)); status=2
      elif [ "$h" = no-compile ] && [ -n "$said" ]; then
        printf "  glob-err %-41s base=%-12s head=%s   (collision rejected)\n" "$label" "$b" "$h"
        globerr=$((globerr + 1))
      elif [ "$h" = no-compile ]; then
        printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
        echo "          the head side failed to build but no diagnostic names \`$name\` —"
        echo "          the probe broke for an unrelated reason and measured nothing"
        incon=$((incon + 1)); status=2
      # The identical hole, on the OTHER side. This used to be `elif [ -n "$said" ]`, which
      # accepted ANY non-no-compile head classification as `glob-ok` the moment rustc said
      # something ambiguity-shaped ANYWHERE in the head log — `upstream-fail`,
      # `bad-witness(...)`, or a `witness=*` these templates cannot produce all read as a pass
      # exactly as readily as a genuine `unreached`. Every one of those means the head side
      # never reached a normal, comparable conclusion, which is the same absence the `$b` check
      # above exists to catch — an attributed ambiguity diagnostic sitting in the log is not
      # evidence of a COMPLETED head-side probe. `h` is now held to the identical two-shape
      # allowlist as `b`: `unreached` is the only shape left once `no-compile` is handled above,
      # because `glob_name`/`glob_macro`'s `drive()` calls only `ran()` and never `reached()`
      # (see the comment above the `$b` check) — a head that actually finished compiling and
      # running can only ever read `unreached` here, never `witness=*`.
      elif [ "$h" = unreached ] && [ -n "$said" ]; then
        printf "  glob-ok %-42s base=%-12s head=%s   (not rejected here; see README)\n" "$label" "$b" "$h"
        globok=$((globok + 1))
      elif [ "$h" = unreached ]; then
        printf "  UNPROBED %-41s base=%-12s head=%s\n" "$label" "$b" "$h"
        echo "          both sides built and rustc said NOTHING about \`$name\` — so this"
        echo "          is not 'the toolchain does not reject it', it is 'no collision was"
        echo "          constructed'. Those are the same absence and must not read as green."
        unprobed=$((unprobed + 1))
        [ "$status" -lt 1 ] && status=1
      else
        printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
        echo "          the head side did not reach a normal compile-or-reject conclusion"
        echo "          (head=$h), so an attributed ambiguity diagnostic left in its log cannot"
        echo "          stand in for a completed head-side probe"
        incon=$((incon + 1)); status=2
      fi
      continue
      ;;
  esac
  # Two more ways a probe can pass while proving nothing. Both were found by asking
  # "what else looks like agreement but is absence?" after the base-no-compile class.
  #
  #  (a) the consumer's own item never ran on the BASE side, so there is no before-state
  #      even though the probe compiled — the drive path errored out before the call;
  #  (b) both sides agree with the consumer's item running, which means the added name was
  #      never a candidate at all. That is what ten of the thirteen glob probes did while
  #      reporting clean, and it is the defect this harness exists for.
  case "$b$h" in
    *unreached*)
      printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
      echo "          the probe ran but control never reached the call under test, so its"
      echo "          witness says nothing about the name"
      incon=$((incon + 1))
      status=2
      continue
      ;;
    *upstream-fail*)
      printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
      echo "          a dependency failed to build, so the probe crate was never compiled"
      echo "          and this row measured nothing"
      incon=$((incon + 1))
      status=2
      continue
      ;;
    *bad-witness*)
      printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
      echo "          a CONSUMER-CALLS or REACHED marker was missing or duplicated, so the"
      echo "          run cannot be interpreted at all"
      incon=$((incon + 1))
      status=2
      continue
      ;;
  esac
  case "$b" in
    witness=1*) ;;
    no-compile) ;;
    *)
      printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
      echo "          the consumer's own item did NOT run on the base side, so the probe"
      echo "          never exercised the name it claims to test"
      incon=$((incon + 1))
      status=2
      continue
      ;;
  esac
  if [ "$b" = no-compile ]; then
    # "The base side did not compile" is an absence, and it has TWO causes — which is the
    # pattern every other false verdict in this harness turned out to be.
    #
    #   1. the probe is broken. No before-state, nothing compared, fatal.
    #   2. the OWNER does not exist on the base ref, because the same diff introduces it.
    #      #147's `RecursionLimitReached` is the first of these: 14 rows whose probe names a
    #      type the base ref has never heard of.
    #
    # The second is not a broken probe and not something a better template can reach. A probe
    # is byte-identical on both sides by construction, and a method call needs a CONCRETE
    # receiver type — so a name on an owner the same release introduces has no consumer call
    # site that could exist before the release, and therefore no before-state to steal. That
    # absence IS the finding. Refusing to say so would either fail the gate forever or push the
    # row onto a subject that does exist, which is the vacuous-probe defect this harness was
    # built after: `Gadget::parse_except` cannot collide with `Ident::parse_except`.
    #
    # FIVE witnesses, and TWO head-side shapes — "fine" is again being asserted by an absence
    # and this file's rule is that each such verdict must produce positive evidence:
    #
    #   * rustc must SAY, on the base side, that it cannot resolve THIS ROW'S OWNER — an
    #     unresolved import / cannot-find / failed-to-resolve diagnostic naming it. A base build
    #     that failed for any other reason is still a broken probe.
    #   * the head side must NOT say the same thing. Otherwise a template that misspells its
    #     owner reports `new-owner` on every run while never having compiled anywhere, which is
    #     the same shape as a probe that tests nothing.
    #   * the head side must have COMPLETED — reached a witness, not merely stayed silent about
    #     the base diagnostic. Silence is what `no-compile`, `upstream-fail`, `bad-witness(...)`
    #     and `unreached` all look like from here just as much as a clean run does: none of them
    #     say "`$owner` is unresolved", so `head_unresolved` reads empty for all of them, and
    #     without this third check any of the four reads as proof the owner is new. A head build
    #     that fails to compile the probe for a reason that has nothing to do with `$owner` — a
    #     typo elsewhere in the same generated file, say — is the sharpest case: it still sets
    #     `$h` to `no-compile`, same as the base side, and no diagnostic anywhere disagrees. This
    #     is the same defect class this whole verdict exists to catch, sitting inside the verdict
    #     itself.
    #   * and a completed head run must show tokora's item TOOK THE CALL, not merely that SOME
    #     call completed. `witness=N` is CONSUMER-CALLS (see the marker comment above this
    #     loop) — attribution, not existence — and `witness=1` means the CONSUMER's own
    #     extension item is what ran, i.e. the probed name on the new owner was never a
    #     candidate this run. A completed run and a resolved owner are both true on a
    #     `witness=1` row, and neither says a collision happened. Accepting any `witness=*`
    #     here made that shape indistinguishable from the one that actually proves it, so a
    #     future template that compiles clean while constructing no real collision would still
    #     score `new-owner`. The check below is therefore an ALLOWLIST of the one shape that is
    #     actual evidence (`witness=0*`) rather than a denylist of the shapes known to be broken
    #     — a denylist is silent about the next broken shape nobody has named yet.
    #
    #   * and the head side must not report the OWNER's import UNUSED. See the fifth witness
    #     below: an inherent item the subject already carried produces `witness=0` exactly as a
    #     new trait item does, and only rustc's `unused import` separates them.
    #
    # And the silent steal is not the only head-side shape that proves a collision. A new owner
    # whose item cannot WIN the receiver walk loses the call to the consumer or ties with it,
    # and a tie is E0034 — rustc naming the collision instead of taking it. That is the
    # `new-owner-err` arm below, and it is why this branch is a case over `$h` rather than a
    # single test.
    #
    # What the row then means is narrow and is printed with it: no pre-existing call site can be
    # stolen. It says nothing about a consumer written AFTER the release, which is out of scope
    # for a two-sided delta harness by construction.
    base_unresolved=""
    head_unresolved=""
    # Captured once per side, matched against the capture — see `new_warnings` above for why:
    # piping straight into `grep -q` lets a SIGPIPE 141 from the first `grep` outrank the
    # second `grep`'s true 0 under pipefail, and this pair is where that inversion would
    # misclassify a resolved-vs-unresolved OWNER, not just a warning annotation. `owner_lines_head`
    # is reused below for `owner_unused` — same owner, same file, one capture.
    owner_lines_base="$(grep -E "(^|[^A-Za-z0-9_])$owner([^A-Za-z0-9_]|$)" "$WORK/out-base.txt" 2>/dev/null)"
    grep -qE "unresolved import|cannot find|failed to resolve|E0432|E0433|E0412" \
      <<<"$owner_lines_base" && base_unresolved=yes
    owner_lines_head="$(grep -E "(^|[^A-Za-z0-9_])$owner([^A-Za-z0-9_]|$)" "$WORK/out-head.txt" 2>/dev/null)"
    grep -qE "unresolved import|cannot find|failed to resolve|E0432|E0433|E0412" \
      <<<"$owner_lines_head" && head_unresolved=yes
    # THE FIFTH WITNESS. `witness=0` is CONSUMER-CALLS, so what it establishes is "the
    # consumer's item did not run" — and for a new owner that is produced by TWO things: the
    # new item taking the call, which is the verdict, and an item of the same name the SUBJECT
    # ALREADY CARRIED taking it, which measures nothing. Measured on the branch that found
    # this: `RecoveryState` is new and declares `is_valid`/`is_error`/`is_missing`, `Ident` has
    # carried inherent `is_valid`/`is_error`/`is_missing` since before the base ref, and an
    # inherent item outranks every trait item at the same step. Three rows compiled, ran,
    # reported `witness=0` and scored `new-owner` over a call that went to a method the base
    # ref already had. Not one of the four witnesses above disagrees with any of them.
    #
    # rustc says it out loud, which is what makes this a check and not a taste: an import that
    # resolved and was never needed is `unused import`. If the head side reports THIS ROW'S
    # OWNER unused, tokora's item on that owner was not a candidate, whatever the witness
    # reads. That is the README's rule — enumerate what else could produce this evidence and
    # exclude each — applied to `witness=0`: the alternative producer is removed rather than
    # the witness complicated.
    #
    # It bills the generator for one thing and the templates must keep paying it: an owner's
    # import may NOT carry `#[allow(unused_imports)]`. A suppressed warning is a witness that
    # cannot fire, and every false green in this file's history was asserted by an absence.
    owner_unused=""
    # Reuses `owner_lines_head`, captured above for `head_unresolved` — same owner, same file,
    # same SIGPIPE hazard the capture already avoids.
    grep -q "unused import" <<<"$owner_lines_head" && owner_unused=yes
    # The head side's LOUD evidence. `new-owner` took exactly ONE head-side shape as proof that
    # a collision was constructed — `witness=0`, the silent steal — and a trait method taking
    # `&self` cannot produce it. The `trait_method` spellings put the consumer's item one pick
    # EARLIER (`self`, by value) or at the SAME pick (`&self`); there is no LATER pick among
    # them, so the outcomes are "the consumer wins" and "rustc refuses to choose". The refusal
    # is E0034, it names the probed item and both candidate impls, and it is strictly stronger
    # evidence than `witness=0` — the compiler REPORTING the collision rather than taking it.
    # Classified as bare `no-compile` it scored INCONCL, so ten of one release's fourteen trait
    # rows were filed "broken probe" over rustc naming the exact collision they were built for.
    #
    # The witness is `glob-err`'s, moved from the glob level to the item level — but NOT its
    # implementation, and the difference is measured rather than assumed. `glob-err` pipes the
    # name-matching lines into an ambiguity-wording test, i.e. both on ONE line, which is how
    # E0659 prints: "`X` is ambiguous". E0034 does not: the code is on the header line, the name
    # is on the label line, and the owner is on a candidate note. Written as one pipe this test
    # matched nothing and every rejected row stayed INCONCL.
    #
    # So it is three conditions over the whole head log, each of them rustc's own words about
    # THIS row, and all three required:
    #
    #   * the error KIND — E0034 / "multiple applicable items in scope";
    #   * the ambiguity's own label naming this row's ITEM — "multiple `<name>` found";
    #   * a candidate note naming this row's OWNER — which is the half that proves TOKORA's item
    #     was one of the two candidates rather than some other pair of the same name.
    #
    # Together they exclude what the README's rule asks to be excluded: a head build that broke
    # elsewhere (no E0034), an ambiguity about a different name (no label), and an ambiguity
    # between two candidates neither of which is tokora's (no owner note). Cargo naming the probe
    # crate is already required to reach `no-compile` at all, so a dependency cannot get here.
    #
    # If a future rustc rewords any of the three, rows go INCONCL and the gate reds. That is the
    # right direction for a wording dependency to fail, and it is why this is three tests rather
    # than one loose one.
    head_rejected=""
    # The first two tests grep a FILE directly and are not piped into anything, so they cannot
    # race. The third IS a pipe — candidate-note lines into a second `grep -qE` — and is
    # captured once for the same reason as every other site in this file: `grep -q` matching
    # early can SIGPIPE the writing `grep` before it finishes, and pipefail would then report
    # that 141 instead of the reader's true 0.
    candidate_lines="$(grep -E "candidate #[0-9]+ is defined in an impl of the trait" \
      "$WORK/out-head.txt" 2>/dev/null)"
    if grep -qE "E0034|multiple applicable items in scope" "$WORK/out-head.txt" 2>/dev/null \
       && grep -qE "multiple \`$name\` found" "$WORK/out-head.txt" 2>/dev/null \
       && grep -qE "(^|[^A-Za-z0-9_])$owner([^A-Za-z0-9_]|$)" <<<"$candidate_lines"; then
      head_rejected=yes
    fi
    if [ -n "$base_unresolved" ] && [ -z "$head_unresolved" ]; then
      case "$h" in
        witness=0*)
          if [ -n "$owner_unused" ]; then
            printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
            echo "          head reports the import of \`$owner\` UNUSED, so tokora's item on it was"
            echo "          never a candidate: the call went to an item of this name the SUBJECT"
            echo "          already carried, which the base ref has too. \`witness=0\` says only that"
            echo "          the CONSUMER's item did not run, and that is all it says here. Give the"
            echo "          row a subject carrying no pre-existing item of this name."
            incon=$((incon + 1))
            status=2
          else
            printf "  new-owner %-40s base=%-12s head=%s   (owner \`%s\` is new)\n" \
              "$label" "$b" "$h" "$owner"
            echo "          rustc cannot resolve \`$owner\` on the base ref and can on head, so this"
            echo "          release introduces the owner as well as the name. There is no consumer"
            echo "          call site that could predate it and nothing to steal."
            newowner=$((newowner + 1))
          fi
          ;;
        witness=*)
          # The consumer's item won the walk. For a PRE-EXISTING owner that is the `ok*` shape
          # below — agreement, justified in writing by the pick order — and it is the SAME fact
          # here, stated about one side because a new owner has no second one. It is a claim
          # about the RECEIVER KINDS these templates declare, not about the name, so it is
          # justified in the same file and staleness-checked by the same reconciliation.
          if grep -qxF "$label" "$NOCOLLIDE"; then
            printf "  new-owner-nc* %-36s base=%-12s head=%s   (earlier pick, justified)\n" \
              "$label" "$b" "$h"
            echo "          \`$owner\` is unresolved on base, so no call site predates the name; on"
            echo "          head the CONSUMER's item claims the call at an EARLIER pick and"
            echo "          tokora's is never a candidate. No collision is constructed at this"
            echo "          spelling, and no_collision.txt states the pick order that does it."
            seen_nocollide="$seen_nocollide $label"
            newownernc=$((newownernc + 1))
          else
            printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
            echo "          head completed a run and resolved \`$owner\`, but \`$h\` is"
            echo "          CONSUMER-CALLS, not existence: a nonzero count means the CONSUMER's"
            echo "          own item took the call, so tokora's new \`$owner\` item was never a"
            echo "          candidate this run. This proves the probe compiles, not that a"
            echo "          collision exists. If the receiver kinds make that the only outcome"
            echo "          this spelling can have, state that pick order in no_collision.txt."
            incon=$((incon + 1))
            status=2
          fi
          ;;
        no-compile)
          if [ -n "$head_rejected" ]; then
            printf "  new-owner-err %-36s base=%-12s head=%s   (collision rejected)\n" \
              "$label" "$b" "$h"
            echo "          rustc cannot resolve \`$owner\` on the base ref, so no consumer call"
            echo "          site predates the name; on head it names \`$name\` and REFUSES to"
            echo "          choose between the candidates. The collision is real and reported —"
            echo "          the disclosed outcome, one side at a time."
            newownererr=$((newownererr + 1))
          else
            printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
            echo "          the owner \`$owner\` is unresolved on the BASE side and head does not"
            echo "          repeat that diagnostic — but head failed to build and NO diagnostic"
            echo "          names \`$name\` ambiguously, so head broke for a reason that is not"
            echo "          this collision. A broken probe on the side that was supposed to"
            echo "          prove the owner is new, not a clean result."
            incon=$((incon + 1))
            status=2
          fi
          ;;
        *)
          printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
          echo "          the owner \`$owner\` is unresolved on the BASE side and head does not"
          echo "          repeat that diagnostic — but head never reached a completed run"
          echo "          (head=$h), so that silence is not evidence head resolved \`$owner\` at"
          echo "          all, let alone ran past it. This is a broken probe on the side that was"
          echo "          supposed to prove the owner is new, not a clean result."
          incon=$((incon + 1))
          status=2
          ;;
      esac
    else
      printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
      echo "          the probe does not compile on the BASE side and no base-side diagnostic"
      echo "          says \`$owner\` is unresolved there — so there is no before-state to"
      echo "          compare against and no evidence the owner is new. This is a broken probe,"
      echo "          not a clean result."
      incon=$((incon + 1))
      status=2
    fi
  elif [ "$b" = "$h_raw" ]; then
    # Agreement means the added name was not a candidate. Sometimes that is the honest
    # answer (a bound genuinely rejects the receiver) — but it is indistinguishable from a
    # probe that constructs no collision, so it has to be justified in writing rather than
    # accepted by default.
    if grep -qxF "$label" "$NOCOLLIDE"; then
      printf "  ok*     %-42s base=%-12s head=%s   (justified)\n" "$label" "$b" "$h"
      seen_nocollide="$seen_nocollide $label"
      okc=$((okc + 1))
    else
      printf "  UNPROBED %-41s base=%-12s head=%s\n" "$label" "$b" "$h"
      echo "          both sides run the CONSUMER's item, so the added name was never a"
      echo "          candidate — either the probe is vacuous or a bound rejects the"
      echo "          receiver. Say which in ci/name_collision/no_collision.txt."
      unprobed=$((unprobed + 1))
      [ "$status" -lt 1 ] && status=1
    fi
  elif [ "$b" != no-compile ] && [ "$h" != no-compile ]; then
    case "$h" in
      *warned*)
        printf "  warned  %-42s base=%-12s head=%s   (diagnostic emitted)\n" "$label" "$b" "$h"
        loud=$((loud + 1))
        ;;
      *)
        silent=$((silent + 1))
        if grep -qxF "$label" "$DISCLOSED"; then
          printf "  silent* %-42s base=%-12s head=%s   (disclosed)\n" "$label" "$b" "$h"
          seen_disclosed="$seen_disclosed $label"
        else
          printf "  SILENT  %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
          echo "          both sides compile and run DIFFERENT programs — no error, no"
          echo "          warning. If this is intended, add it to ci/name_collision/"
          echo "          disclosed.txt AND to the CHANGELOG's silent section."
          newsilent=$((newsilent + 1))
          [ "$status" -lt 1 ] && status=1
        fi
        ;;
    esac
  else
    printf "  loud    %-42s base=%-12s head=%s   (reported by rustc)\n" "$label" "$b" "$h"
    loud=$((loud + 1))
  fi
done < "$WORK/plan.tsv"

# `owner-introduced-here` is the TOTAL of the three shapes a new owner can reach, with the two
# that are not silent steals named in the parenthetical — the same convention as
# `silent (N undisclosed)`, where the bracket is a subset and the remainder is the plain case.
newownerall=$((newowner + newownererr + newownernc))
echo "name-collision: $total probe(s) — $okc justified-no-collision, $loud loud, $silent silent ($newsilent undisclosed), $newownerall owner-introduced-here ($newownererr rejected, $newownernc earlier-pick), $incon inconclusive, $unprobed UNPROBED, $fatal FATAL; globs: $globerr rejected, $globok not-rejected"

# A baseline entry that no longer reproduces is stale. Left unchecked it becomes a rubber
# stamp for a probe that stopped running at all — the failure mode of every allowlist.
#
# But "did not reproduce" and "was never attempted" are the same absence, and only the first
# is a finding. The inventory is a DELTA: a PR is probed for the names IT adds, so a baseline
# row recorded by an earlier release is not in the plan and cannot reproduce, however healthy
# it is. Reconciling against every row therefore fired on the first name-adding PR after the
# release that wrote the baseline — #132 adds one name and saw all four disclosed rows
# reported STALE, none of which it touched.
#
# So a row is reconciled only when this run PLANNED it. In scope and unseen is stale, and
# still fatal; out of scope is reported and carried. Note what that costs and why it is not
# avoidable here: a baseline row is only ever reconcilable on a run whose diff adds its name,
# i.e. the release that recorded it. Probing it on every run is not the fix — the name exists
# on BOTH sides by then, the two sides agree, and the row would score UNPROBED rather than
# reproduce. A two-sided delta harness cannot re-litigate a name that is no longer new.
outscope_d=0
outscope_n=0
while read -r line; do
  case "$line" in ''|\#*) continue ;; esac
  if ! grep -qxF "$line" "$WORK/planned.txt"; then
    outscope_d=$((outscope_d + 1))
    echo "  n/a     $line is in disclosed.txt and OUT OF SCOPE for this diff (not probed)"
    continue
  fi
  case " $seen_disclosed " in
    *" $line "*) ;;
    *)
      echo "  STALE   $line is in disclosed.txt, WAS probed by this run, and did not" >&2
      echo "          reproduce — remove it" >&2
      [ "$status" -lt 1 ] && status=1
      ;;
  esac
done < "$DISCLOSED"

while read -r line; do
  case "$line" in ''|\#*) continue ;; esac
  if ! grep -qxF "$line" "$WORK/planned.txt"; then
    outscope_n=$((outscope_n + 1))
    echo "  n/a     $line is in no_collision.txt and OUT OF SCOPE for this diff (not probed)"
    continue
  fi
  case " $seen_nocollide " in
    *" $line "*) ;;
    *)
      echo "  STALE   $line is in no_collision.txt, WAS probed by this run, and did not" >&2
      echo "          reproduce — remove it" >&2
      [ "$status" -lt 1 ] && status=1
      ;;
  esac
done < "$NOCOLLIDE"

if [ "$outscope_d" -ne 0 ] || [ "$outscope_n" -ne 0 ]; then
  echo "name-collision: $outscope_d disclosed and $outscope_n no-collision row(s) were out of"
  echo "name-collision: scope: this diff does not add their names, so this run had no occasion"
  echo "name-collision: to construct them. That is not evidence they still hold."
fi

if [ "$status" -ne 0 ]; then
  echo "name-collision: RUN FAILED (exit $status) — the verdicts above are incomplete;" >&2
  echo "name-collision: do NOT read this run as evidence of anything." >&2
elif [ "$newsilent" -eq 0 ] && [ "$unprobed" -eq 0 ]; then
  # Deliberately narrow wording. A pass means no silent steal OF THE ONE CONSUMER SHAPE
  # this harness can express — an extension item whose signature DIFFERS from tokora's. A
  # consumer whose signature matches loses the call silently and is not probed here at all.
  echo "name-collision: PASS — and read what that does NOT mean:"
  echo "name-collision:   * probes use a consumer whose signature DIFFERS from ours. In"
  echo "name-collision:     the inherent-method and associated-function categories that"
  echo "name-collision:     consumer takes no arguments, so for a name that takes some the"
  echo "name-collision:     head side fails on the probe own arity (E0061) and that loud"
  echo "name-collision:     verdict measures nothing about the name. The trait-method"
  echo "name-collision:     probes DO pass real arguments; their rows are genuine."
  echo "name-collision:   * a consumer whose signature MATCHES ours can steal the call"
  echo "name-collision:     silently even with the return used. That shape is not probed."
  echo "name-collision:   * glob rows carry no witness; their verdict is compile-status"
  echo "name-collision:     only, and which class it lands in depends on the toolchain."
  echo "name-collision:   * a new-owner row says only that no call site could PREDATE the"
  echo "name-collision:     name. A two-sided delta cannot speak about a consumer written"
  echo "name-collision:     after the release, on either side of that line."
  echo "name-collision:   * new-owner-err says rustc REFUSED this probe's collision, not that"
  echo "name-collision:     the name cannot be taken silently. The trait spellings declare"
  echo "name-collision:     \`self\` and \`&self\`, which against a tokora \`&self\` item are the"
  echo "name-collision:     EARLIER pick and the SAME pick; the \`&mut self\` consumer that"
  echo "name-collision:     would sit at a LATER one and lose the call quietly is a shape"
  echo "name-collision:     these templates do not generate. So on such an owner the silent"
  echo "name-collision:     new-owner verdict is unreachable BY CONSTRUCTION, and a run of"
  echo "name-collision:     new-owner-err rows is not evidence that no silent shape exists."
  echo "name-collision:   * new-owner-nc* is a statement about RECEIVER KINDS, not about the"
  echo "name-collision:     name: the by-value consumer keeps its call. The same names go"
  echo "name-collision:     loud the moment the consumer writes \`&self\`, which is what the"
  echo "name-collision:     new-owner-err rows on those same names report."
  echo "name-collision: This is a regression detector, not a proof of absence."
fi
exit "$status"
