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
git -C "$ROOT" archive "$BASE_REF" | tar -x -C "$WORK/base" || {
  echo "name-collision: FATAL — could not extract base ref $BASE_REF" >&2; exit 2; }
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
# An associated TYPE or CONST on a trait. `gen_probe.py` has no template and exits non-zero
# by name; the row exists so that refusal is attached to the item rather than the item being
# dropped from the plan. Dropping it is the failure mode this whole harness was built after.
for n in d["trait_other_names"]:
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
  if [ -s "$WORK/warn-head.txt" ] \
     && comm -13 "$WORK/warn-base.txt" "$WORK/warn-head.txt" 2>/dev/null \
        | grep -qE "(^|[^A-Za-z0-9_])$name([^A-Za-z0-9_]|$)"; then
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
      said=""
      grep -E "(^|[^A-Za-z0-9_])$name([^A-Za-z0-9_]|$)" "$WORK/out-head.txt" 2>/dev/null \
        | grep -qiE "ambiguous|E0659" && said=yes
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
      elif [ -n "$said" ]; then
        printf "  glob-ok %-42s base=%-12s head=%s   (not rejected here; see README)\n" "$label" "$b" "$h"
        globok=$((globok + 1))
      else
        printf "  UNPROBED %-41s base=%-12s head=%s\n" "$label" "$b" "$h"
        echo "          both sides built and rustc said NOTHING about \`$name\` — so this"
        echo "          is not 'the toolchain does not reject it', it is 'no collision was"
        echo "          constructed'. Those are the same absence and must not read as green."
        unprobed=$((unprobed + 1))
        [ "$status" -lt 1 ] && status=1
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
    printf "  INCONCL %-42s base=%-12s head=%s\n" "$label" "$b" "$h"
    echo "          the probe does not compile on the BASE side, so there is no before-state"
    echo "          to compare against — this is a broken probe, not a clean result"
    incon=$((incon + 1))
    status=2
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

echo "name-collision: $total probe(s) — $okc justified-no-collision, $loud loud, $silent silent ($newsilent undisclosed), $incon inconclusive, $unprobed UNPROBED, $fatal FATAL; globs: $globerr rejected, $globok not-rejected"

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
  echo "name-collision: This is a regression detector, not a proof of absence."
fi
exit "$status"
