#!/bin/bash
#
# Self-test for ci/changelog_structure.sh.
#
# The gate it tests exists because CHANGELOG.md broke in production twice, and both times the
# required job was GREEN. Since then the gate itself has been caught passing on planted defects
# three review rounds running — first three spellings, then four, then six more — every one of
# which GitHub's own renderer turns into a real heading, a real list item, a real fragment
# target or a real link. Same class each time, relocated. A check nobody has watched fail is
# not a check, and a check whose coverage is a list of the spellings somebody happened to think
# of is a list, not a rule.
#
# So this file has three parts, and the third is the one that matters.
#
#   A. GREEN CASES — well-formed but unusual files that must PASS. These are the false-positive
#      guards. "Everything reds" is not the same as "everything is checked", and a refuse-by-
#      default gate fails in this direction: `Element<A>` inside a code span must not be taken
#      for an anchor tag, and `### C#` must not be taken for a closing sequence.
#
#   B. STRUCTURAL MUTATIONS — one real defect injected at a time (a heading eaten, an anchor
#      renamed, every link deleted). These cover the accidents rather than the spellings.
#
#   C. THE SPELLING TABLE — the structural fix for the recurrence. Every CommonMark spelling
#      that reaches a heading, a list label, a fragment target or a link, each carrying the
#      disposition the gate owes it: `resolved` (recognised, and resolved against the anchor
#      table) or `refused` (rejected by name, with the canonical form printed). The assertion is
#      that it gets ONE OF THE TWO — never a green, which would mean the spelling was silently
#      ignored. Every row was put through `gh api /markdown` before it was added: each renders.
#
#      A fixed mutation list only ever covers what someone already imagined, which is exactly
#      why two rounds each found more. A table turns "did we think of this one?" from a question
#      about the reviewer's imagination into a row. When a new spelling turns up, add the row —
#      but read clause (e) of ci/changelog_structure.sh first, because a spelling that
#      refuse-by-default should already have caught means the inversion is incomplete somewhere,
#      and another row would paper over that.
#
# Every case runs under every awk on the machine and must produce byte-identical stdout AND
# stderr under all of them. ubuntu-latest runs mawk, macOS gives you BSD awk, gawk is common on
# both; the mutation half is load-bearing, because the OK path never writes to /dev/stderr and
# only a red exercises the diagnostic formatting where implementations could diverge.
#
# Usage:  bash ci/changelog_structure_selftest.sh [CHANGELOG.md] [ci/changelog_structure.sh]
#         CHANGELOG_SELFTEST_AWKS="awk mawk" bash ci/changelog_structure_selftest.sh

set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)

FILE=${1:-$root/CHANGELOG.md}
GATE=${2:-$here/changelog_structure.sh}

[ -f "$FILE" ] || { echo "selftest: $FILE does not exist" >&2; exit 1; }
[ -f "$GATE" ] || { echo "selftest: $GATE does not exist" >&2; exit 1; }

# ── the awk implementations to test under ────────────────────────────────────────────────────
# Every one found, unless the caller names a set. The gate calls a bare `awk`, so each is
# selected by putting a one-line shim first on PATH — the same way CI would pick one.
awks=${CHANGELOG_SELFTEST_AWKS:-}
if [ -z "$awks" ]; then
  for cand in awk mawk gawk nawk original-awk; do
    p=$(command -v "$cand" 2>/dev/null) || continue
    [ -n "$p" ] && awks="$awks $p"
  done
fi
set -- $awks
[ $# -gt 0 ] || { echo "selftest: found no awk to test with" >&2; exit 1; }
awks="$*"

work=$(mktemp -d "${TMPDIR:-/tmp}/changelog-selftest.XXXXXX") || exit 1
trap 'rm -rf "$work"' EXIT INT TERM

awkcount=0
for a in $awks; do
  awkcount=$((awkcount + 1))
  mkdir -p "$work/shim$awkcount"
  printf '#!/bin/sh\nexec %s "$@"\n' "$a" > "$work/shim$awkcount/awk"
  chmod +x "$work/shim$awkcount/awk"
done

echo "changelog-structure selftest"
echo "  gate      $GATE"
echo "  changelog $FILE"
i=0
for a in $awks; do
  i=$((i + 1))
  echo "  awk #$i     $a"
done

fails=0
total=0
rc_disp=""

# ── the runner ───────────────────────────────────────────────────────────────────────────────
# $1 name  $2 mutant  $3 expected exit (0 | 1 meaning nonzero)  $4 marker that must appear in
# stderr  $5 marker that must NOT appear  $6 what to say when a case that must red comes back
# green  $7 a second marker that must also appear, for rows that assert WHICH line was
# diagnosed.  Exit codes are captured directly, never through a pipe.
run_case() {
  rc_name=$1; rc_mut=$2; rc_want=$3; rc_marker=$4; rc_neg=$5; rc_greenmsg=$6; rc_marker2=${7:-}
  total=$((total + 1))
  problems=""; greened=0
  k=0
  for a in $awks; do
    k=$((k + 1))
    PATH="$work/shim$k:$PATH" bash "$GATE" "$rc_mut" > "$work/$rc_name.$k.out" 2> "$work/$rc_name.$k.err"
    rc=$?
    if [ "$rc_want" = 0 ]; then
      [ "$rc" -eq 0 ] || problems="$problems exit=$rc(awk#$k,expected-0)"
    else
      if [ "$rc" -eq 0 ]; then greened=1; problems="$problems exit=0(awk#$k)"; fi
    fi
    if [ -n "$rc_marker" ] && ! grep -qF -- "$rc_marker" "$work/$rc_name.$k.err"; then
      problems="$problems no-marker(awk#$k)"
    fi
    if [ -n "$rc_marker2" ] && ! grep -qF -- "$rc_marker2" "$work/$rc_name.$k.err"; then
      problems="$problems wrong-line-diagnosed(awk#$k)"
    fi
    if [ -n "$rc_neg" ] && grep -qF -- "$rc_neg" "$work/$rc_name.$k.err"; then
      problems="$problems unwanted-refusal(awk#$k)"
    fi
    if [ "$k" -gt 1 ]; then
      cmp -s "$work/$rc_name.1.out" "$work/$rc_name.$k.out" || problems="$problems stdout-differs(awk#1,awk#$k)"
      cmp -s "$work/$rc_name.1.err" "$work/$rc_name.$k.err" || problems="$problems stderr-differs(awk#1,awk#$k)"
    fi
  done

  if [ -z "$problems" ]; then
    if [ -n "$rc_marker" ]; then
      printf '%-26s %-9s %-9s %s\n' "$rc_name" "$rc_disp" "$awkcount agree" "ok — $rc_marker"
    else
      printf '%-26s %-9s %-9s %s\n' "$rc_name" "$rc_disp" "$awkcount agree" "ok — passes, as it must"
    fi
  else
    if [ "$greened" = 1 ]; then
      printf '%-26s %-9s %-9s %s\n' "$rc_name" "$rc_disp" "-" "FAIL — $rc_greenmsg"
    else
      printf '%-26s %-9s %-9s %s\n' "$rc_name" "$rc_disp" "-" "FAIL —$problems"
      sed -n '1,3p' "$work/$rc_name.1.err" | sed 's/^/         | /'
    fi
    fails=$((fails + 1))
  fi
}

anchor_id_of() {
  awk '/^<a id="[^"]*"><\/a>[ \t]*$/ {
         id = $0; sub(/^<a id="/, "", id); sub(/"><\/a>[ \t]*$/, "", id); print id; exit }' "$1"
}

# Insert literal lines just below the first `### ` heading and the blank line under it. That
# spot is inside a section, below a heading, and clear of the anchor geometry (anchor, blank,
# heading), so an injection there exercises the spelling and nothing else.
inject() {
  printf '%b\n' "$1" > "$work/inject.txt"
  awk -v injf="$work/inject.txt" '
    { L[NR] = $0 }
    END {
      for (i = 1; i <= NR; i++) if (!h && L[i] ~ /^### /) h = i
      for (i = 1; i <= NR; i++) {
        print L[i]
        if (i == h + 1) {
          while ((getline ln < injf) > 0) print ln
          close(injf)
          print ""
        }
      }
    }' "$2" > "$3"
}

# ── Part A green cases and Part B structural mutations ───────────────────────────────────────
generate() {
  case "$1" in
  g00-control)             cp "$2" "$3" ;;

  g01-indented-fence)
    # An indented fence is READ, not refused, and what it wraps stays invisible to the checks.
    inject '   \x60\x60\x60\n  ### inside an indented fence, so not a heading\n   \x60\x60\x60' "$2" "$3" ;;

  g02-refdef-resolves)
    # The reference-style extractor resolves; it does not simply refuse everything it sees.
    cp "$2" "$3"; printf '\n[resolves]: #%s\n' "$(anchor_id_of "$2")" >> "$3" ;;

  g03-canonical-link-resolves)
    cp "$2" "$3"; printf '\nsee [resolves](#%s)\n' "$(anchor_id_of "$2")" >> "$3" ;;

  g04-long-fence)
    # A four-backtick fence is not closed by the three backticks inside it.
    inject '\x60\x60\x60\x60\n\x60\x60\x60\n### inside the outer fence, so not a heading\n\x60\x60\x60\n\x60\x60\x60\x60' "$2" "$3" ;;

  g05-generic-lt-a)
    # FALSE-POSITIVE GUARD for the raw-HTML refusal. `Element<A>` lowercases to `<a>`, and the
    # changelog really does carry it — inside a code span that spans two lines, which GitHub
    # renders as one <code>. A refusal keyed on `<a` reds the real file on this.
    inject 'A line with \x60Element<A> + Syntax<B>\x60 and \x60Node<A>\x60 in code spans.' "$2" "$3" ;;

  g06-hash-inside-heading)
    # FALSE-POSITIVE GUARD for the closing-sequence refusal: a `#` with no whitespace before it
    # is part of the text, not a closing sequence. GitHub renders `### C#` as `C#`.
    inject '### Injected C# and F# heading' "$2" "$3" ;;

  g07-thematic-break)
    # FALSE-POSITIVE GUARD for the setext refusal: after a blank line `---` is a thematic
    # break, not an underline, and it makes no heading.
    inject '\n---\n' "$2" "$3" ;;

  g08-external-destinations)
    # FALSE-POSITIVE GUARD for the destination refusal: only destinations that reach INTO this
    # document are the gate's business. Angle brackets and a title are fine on an external one.
    inject 'see [a](https://example.com "title") and [b](<https://example.com>)' "$2" "$3" ;;

  g09-prose-attributes)
    # MANDATORY GUARD for the refuse-by-default over-reach found in R3 review. This changelog
    # documents an HTML-adjacent API, so an entry has every right to name these attributes in
    # prose. The old rule refused the attribute anywhere on the line and made such an entry
    # unmergeable. Bare prose, deliberately: inline code passed even then, but only because a
    # backtick is not the whitespace the old pattern wanted in front of the name.
    inject 'The href="#x" attribute and the id= form and the name= form are now handled.\nAlso \x60href="#y"\x60 and \x60id=\x60 inside code spans.' "$2" "$3" ;;

  g10-refdef-split-external)
    # MANDATORY GUARD: `[docs]:` over an external URL is a legitimate reference definition.
    # Only a continuation that normalizes to an in-document `#target` is refused.
    inject '[docs]:\nhttps://example.com\n\nsee [docs] here' "$2" "$3" ;;

  g11-comment-tail-not-markdown)
    # GUARD against the tempting wrong fix for the comment-prefix hole. The tail after `-->` is
    # inside an HTML block: it passes through as raw HTML, so `<a href=…>` there is a live link
    # — but `### H` and `1. x` there are literal text, NOT a heading and NOT a list item.
    # Checked against `gh api /markdown`. Scanning that tail as Markdown would invent a red.
    inject '<!-- n -->### Not A Heading\n<!-- n -->1. not a list item\n<!-- n -->[x](#no-such-anchor)' "$2" "$3" ;;

  g12-anchor-above-h2)
    # Keeps the `## ` arm of the anchor-name rule alive: above a level-2 heading the expected
    # id is the section key itself. No anchor in the real file exercises it. Appended at EOF
    # instead of going through the shared `inject()` helper above: `inject()` splices right
    # after the live file's first `### ` heading, but this payload OPENS a `## ` section of its
    # own, and a `## ` heading re-sections everything below the splice point — down to the next
    # real `## ` — into it. Spliced after `### Changed (breaking)` under `## Unreleased`, that
    # swallowed the rest of Unreleased, including its own later `### Added`, colliding the two
    # into one duplicate-heading red — so a real content addition (the `cast::token_any` /
    # `cast::tokens` entry, #160) turned this pass-case red on a document it never touched. At
    # EOF the synthetic section has nothing following it to swallow, so it can only ever
    # exercise the anchor-above-h2 rule it exists to test.
    cp "$2" "$3"
    printf '\n<a id="0.9.0"></a>\n\n## 0.9.0 (2026-09-09)\n\n### Added\n\nan entry\n' >> "$3" ;;

  g13-inline-comment-html)
    # MANDATORY GUARD. An inline comment is stripped by GitHub — `text <!-- <div id="x"> --> m`
    # renders as `<p>text  m</p>`, declaring no fragment — but the comment SKIP path only ever
    # engaged for a line beginning `<!--`, so the scanners ran over the raw line and called the
    # commented-out tag live HTML. A changelog that cannot show disabled markup is a changelog
    # that blocks legitimate merges.
    inject 'text <!-- <div id="x"></div> --> more\ntail <!-- <a href="#nope">y</a> --> end' "$2" "$3" ;;

  g14-code-span-html)
    # MANDATORY GUARD. A backticked tag is a code literal, not markup. This changelog documents
    # an HTML-adjacent API and has every reason to quote one. Same for link syntax shown as an
    # example: the collector must not chase a destination that renders as text.
    inject 'Use \x60<a href="#x">\x60 for that.\nThe \x60[x](#nope)\x60 form documents the syntax.\nAnd \x60<div id="y">\x60 too.' "$2" "$3" ;;

  m11-anchor-above-nonheading)
    # An anchor whose target is `##NotAHeading` — a paragraph, because GitHub wants the space —
    # so the anchor names nothing. The id is deliberately the one the gate WANTS, because the
    # placement rule and the naming rule are two arms of one check and a red on the naming arm
    # proves nothing about the placement arm. That is how this hole survived a first review.
    inject '<a id="notaheading"></a>\n\n##NotAHeading\n\nbody text' "$2" "$3" ;;

  m12-anchor-above-h3-nospace)
    inject '<a id="unreleased-notaheading"></a>\n\n###NotAHeading\n\nbody text' "$2" "$3" ;;

  m01-heading-eaten)
    # Production failure 1: a merge eats a heading and leaves its anchor behind.
    awk '{ L[NR] = $0 }
         END { for (i = 1; i <= NR; i++) if (!a && L[i] ~ /^<a id="[^"]*"><\/a>[ \t]*$/) a = i
               for (i = 1; i <= NR; i++) if (i != a + 2) print L[i] }' "$2" > "$3" ;;

  m02-dup-heading)
    awk '{ L[NR] = $0 }
         END { for (i = 1; i <= NR; i++) if (!a && L[i] ~ /^<a id="[^"]*"><\/a>[ \t]*$/) a = i
               for (i = 1; i <= NR; i++) {
                 print L[i]
                 if (i == a + 2) { print ""; print L[a + 2] } } }' "$2" > "$3" ;;

  m03-anchor-typo)
    awk '{ L[NR] = $0 }
         END { for (i = 1; i <= NR; i++) if (!a && L[i] ~ /^<a id="[^"]*"><\/a>[ \t]*$/) a = i
               for (i = 1; i <= NR; i++) {
                 if (i == a) { t = L[i]; sub(/^<a id="/, "<a id=\"x-", t); print t }
                 else print L[i] } }' "$2" > "$3" ;;

  m04-no-blank)
    awk '{ L[NR] = $0 }
         END { for (i = 1; i <= NR; i++) if (!a && L[i] ~ /^<a id="[^"]*"><\/a>[ \t]*$/) a = i
               for (i = 1; i <= NR; i++) { print L[i]; if (i == a) print "not blank" } }' "$2" > "$3" ;;

  m05-no-anchors)
    awk '$0 !~ /^<a id="[^"]*"><\/a>[ \t]*$/' "$2" > "$3" ;;

  m06-no-links)
    awk '{ gsub(/\]\(#[^)]*\)/, "](https://x)"); print }' "$2" > "$3" ;;

  m07-fence-swallows-all)
    # The positive control for (e)/(f): a fence at the top hides the entire document, and every
    # check above would otherwise pass on nothing. Five backticks, so the file's own
    # three-backtick fences cannot close it and leave part of the document visible.
    { printf '`````\n'; cat "$2"; } > "$3" ;;

  m08-dup-label)
    awk '{ L[NR] = $0 }
         END { for (i = 1; i <= NR; i++) if (!f && L[i] ~ /^[0-9]+\. /) f = i
               for (i = f; i >= 1; i--) if (L[i] ~ /^### /) { h = i; break }
               for (i = 1; i <= NR; i++) {
                 print L[i]
                 if (i == h + 1) { print L[f]; print "" } } }' "$2" > "$3" ;;

  m09-dup-section-key)
    awk '{ L[NR] = $0 }
         END { for (i = 1; i <= NR; i++) if (!p && L[i] ~ /^## /) p = i
               for (i = 1; i <= NR; i++) {
                 print L[i]
                 if (i == p + 1) { print L[p]; print "" } } }' "$2" > "$3" ;;

  m10-preamble-dangling-link)
    # Above the first `## `, where the section walk has nothing to walk. Whether a link is
    # checked must not depend on where in the file it sits.
    awk '{ L[NR] = $0 }
         END { for (i = 1; i <= NR; i++) if (!h && L[i] ~ /^# /) h = i
               for (i = 1; i <= NR; i++) {
                 print L[i]
                 if (i == h) { print ""; print "a preamble link: [x](#no-such-anchor)" } } }' "$2" > "$3" ;;

  *) echo "selftest: no generator for $1" >&2; return 1 ;;
  esac
}

structural=$(cat <<'EOF'
g00-control|0|
g01-indented-fence|0|
g02-refdef-resolves|0|
g03-canonical-link-resolves|0|
g04-long-fence|0|
g05-generic-lt-a|0|
g06-hash-inside-heading|0|
g07-thematic-break|0|
g08-external-destinations|0|
g09-prose-attributes|0|
g10-refdef-split-external|0|
g11-comment-tail-not-markdown|0|
g12-anchor-above-h2|0|
g13-inline-comment-html|0|
g14-code-span-html|0|
m01-heading-eaten|1|does not sit above a heading
m02-dup-heading|1|duplicate heading in
m03-anchor-typo|1|does not name the heading it sits above
m04-no-blank|1|is not followed by a blank line
m05-no-anchors|1|anchor declared in
m06-no-links|1|no in-document link
m07-fence-swallows-all|1|not one line of
m08-dup-label|1|repeated numbered label
m09-dup-section-key|1|sections share the key
m10-preamble-dangling-link|1|resolves to no declared anchor
m11-anchor-above-nonheading|1|does not sit above a heading
m12-anchor-above-h3-nospace|1|does not sit above a heading
EOF
)

# ── Part C: the spelling table ───────────────────────────────────────────────────────────────
# name | disposition | literal | extra   (\n newline, \x60 backtick, \x20 space, \t tab)
#
# `extra` is an optional second substring the diagnostic must contain. It exists because a
# row can red for the right reason on the wrong line: `<a` split over two lines reds at the
# `<a` line via the bare-anchor rule and never demonstrates that an attribute on the SECOND
# line is keyed at all. The `<div` split row asserts the echoed line is the attribute one.
#
# Use \x20 for a space, never octal \040: printf %b reads up to three octal digits, so
# `\0401. alpha` becomes ` . alpha` and the row silently stops testing what it names.
# The table caught exactly that when it was first run — the harness, not the gate.
#
# `resolved` rows aim at `#no-such-anchor`, which nothing declares, so a row that really is
# resolved must red as unresolved. `refused-*` rows must red by name. A row that goes GREEN was
# silently ignored — that is the finding this table exists to produce.
spellings=$(cat <<'EOF'
inline-canonical|resolved|see [x](#no-such-anchor)
refdef-canonical|resolved|[phantom]: #no-such-anchor
dest-angle-brackets|refused-dest|see [x](<#no-such-anchor>)
dest-padded|refused-dest|see [x]( #no-such-anchor )
dest-with-title|refused-dest|see [x](#no-such-anchor "t")
refdef-angle-brackets|refused-refdef|[phantom]: <#no-such-anchor>
refdef-split-indoc|refused-refdefsplit|[phantom]:\n#no-such-anchor
html-href|refused-html|<a href="#no-such-anchor">x</a>
html-href-uppercase|refused-html|<a HREF="#no-such-anchor">x</a>
html-id-uppercase|refused-html|<a ID="hijack"></a>
html-id-extra-attrs|refused-html|<a class="q" id="hijack"></a>
html-name-attr|refused-html|<a name="hijack">x</a>
html-div-id|refused-html|<div id="hijack">x</div>
html-span-id|refused-html|<span id="hijack">x</span>
html-after-code-span|refused-html|a \x60code\x60 span then <div id="hijack"></div>
html-after-lone-backtick|refused-html|an unclosed \x60 backtick then <div id="hijack"></div>
html-after-inline-comment|refused-html|<!-- c --> then <div id="hijack"></div>
comment-prefix-anchor|refused-html|<!-- n --><a href="#no-such-anchor">x</a>
comment-prefix-div-id|refused-html|<!-- n --><div id="hijack"></div>
comment-close-tail|refused-html|<!-- a\nb --><div id="hijack"></div>
html-tag-split-anchor|refused-html|<a\nid="hijack"></a>
html-tag-split-div|refused-html|<div\nid="hijack"></div>|id="hijack"></div>
html-anchor-indented|refused-html|\x20\x20<a id="hijack"></a>
html-anchor-two-spaces|refused-html|<a\x20\x20id="hijack"></a>
marker-paren|refused-marker|1) alpha
marker-indented|refused-marker|\x20\x201. alpha
marker-tab-after|refused-marker|1.\talpha
marker-empty|refused-marker|1.
heading-indented|refused-heading|\x20\x20### Injected Heading
heading-closing-seq|refused-heading|### Injected Heading ###
heading-trailing-space|refused-heading|### Injected Heading\x20
heading-trailing-tab|refused-heading|### Injected Heading\t
heading-tab-after-hash|refused-heading|###\tInjected Heading
heading-two-spaces|refused-heading|###\x20\x20Injected Heading
heading-no-text|refused-heading|###
setext-underline-h2|refused-setext|An injected paragraph line\n---
setext-underline-h1|refused-setext|An injected paragraph line\n===
fence-unterminated|refused-region|\x60\x60\x60\x60\x60
comment-unterminated|refused-region|<!--
EOF
)

marker_for() {
  case "$1" in
    resolved)            echo "resolves to no declared anchor" ;;
    refused-dest)        echo "links into this document in a spelling" ;;
    refused-refdef)      echo "defines a reference into this document" ;;
    refused-refdefsplit) echo "destination reaches into this" ;;
    refused-html)        echo "carries raw HTML that GitHub turns into" ;;
    refused-marker)      echo "is a numbered list item GitHub renders" ;;
    refused-heading)     echo "is a heading GitHub renders" ;;
    refused-setext)      echo "is a setext heading underline" ;;
    refused-region)      echo "is never closed" ;;
  esac
}

# ── run ──────────────────────────────────────────────────────────────────────────────────────
echo
echo "A/B — green cases, then structural mutations"
printf '%-26s %-9s %-9s %s\n' "CASE" "EXPECT" "AWKS" "RESULT"
printf '%-26s %-9s %-9s %s\n' "--------------------------" "---------" "---------" "------"

while IFS='|' read -r name want marker; do
  [ -n "$name" ] || continue
  mut="$work/$name.md"
  if ! generate "$name" "$FILE" "$mut"; then
    printf '%-26s %-9s %-9s %s\n' "$name" "-" "-" "FAIL (generator)"
    fails=$((fails + 1)); total=$((total + 1))
    continue
  fi
  if [ "$want" = 0 ]; then rc_disp="pass"; else rc_disp="red"; fi
  run_case "$name" "$mut" "$want" "${marker:-}" "" "went green"
done <<EOF
$structural
EOF

echo
echo "C — spelling table: every rendered spelling is resolved or refused, never ignored"
printf '%-26s %-9s %-9s %s\n' "SPELLING" "DISPOSN" "AWKS" "RESULT"
printf '%-26s %-9s %-9s %s\n' "--------------------------" "---------" "---------" "------"

while IFS='|' read -r name disp literal extra; do
  [ -n "$name" ] || continue
  mut="$work/$name.md"
  inject "$literal" "$FILE" "$mut"
  marker=$(marker_for "$disp")
  if [ -z "$marker" ]; then
    printf '%-26s %-9s %-9s %s\n' "$name" "$disp" "-" "FAIL (unknown disposition)"
    fails=$((fails + 1)); total=$((total + 1))
    continue
  fi
  # A canonical spelling must be resolved, never refused: assert the refusal wording is absent.
  neg=""
  if [ "$disp" = resolved ]; then neg="spelling this file does not use"; rc_disp="resolved"
  else rc_disp="refused"; fi
  run_case "$name" "$mut" 1 "$marker" "$neg" \
    "SILENTLY IGNORED — the gate does not see this spelling at all" "${extra:-}"
done <<EOF
$spellings
EOF

echo
if [ "$fails" -eq 0 ]; then
  echo "changelog-structure selftest: OK — $total case(s), $awkcount awk binary(ies), stdout and stderr byte-identical across all of them."
  exit 0
fi
echo "changelog-structure selftest: FAIL — $fails of $total case(s)." >&2
exit 1
