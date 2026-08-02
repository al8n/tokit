#!/bin/bash
#
# Structural check for CHANGELOG.md — every section, and the whole cross-reference graph.
#
# The changelog is a load-bearing release artifact that no compiler, linter or test reads.
# It has already failed in production twice.
#
#   1. A rebase resolved `CHANGELOG.md` as a verbatim union — textually correct, no conflict
#      markers — and silently dropped a `### Changed (breaking)` heading, filing six breaking
#      changes under `### Performance`. Every other gate passed, because nothing reads a
#      changelog. This script was written for that.
#
#   2. A merge then prepended a `## Unreleased` section. That did not touch one byte of the
#      released section below it, and it broke two things at once. This script used to check
#      "the newest `## ` section only", so the window moved off the 2200-line release section
#      onto a one-item stub and the required job went green on a section nobody was editing.
#      And GitHub numbers duplicate heading slugs in *document order*, so the new
#      `### Changed (breaking)` took `#changed-breaking` and the 53 cross-references in the
#      release section below silently re-pointed at an empty stub.
#
# Both are the same defect: a structure check whose answer depends on where a section sits in
# the file. So nothing here depends on position any more.
#
#   (a) no duplicate heading (levels 3–6) within a `## ` section — EVERY section, not the
#       newest one. A window of one section is a window an insertion can move. Released
#       sections are frozen, so checking them costs nothing once they pass; a red on one means
#       something changed frozen history, which is exactly what a merge-blocker is for.
#
#   (b) numbered-list labels are unique within each `## ` section. This is the detector for
#       failure 1: when a heading is eaten, its numbered items land under a neighbour and the
#       labels collide. (Gaps in a numbering run are NOT an error. Withdrawing an entry while
#       leaving the numbers alone is the correct move once cross-references exist, and it
#       leaves a gap. `0.8.0` has one, at 34, from the release's renumbering pass — no entry
#       was lost; label 34 exists in no commit in this history.)
#
#   (c) every section is non-empty and carries at least one `### ` heading, and at least one
#       section exists. Positive control: without it a green result could mean "the extraction
#       matched nothing" rather than "the file is well formed". A check that cannot fail is
#       not a check.
#
#   (d) cross-reference integrity, and it is position-independent by construction:
#
#         · every in-document link must resolve to an explicitly declared anchor,
#           `<a id="target"></a>`. Two spellings are resolved — inline `](#target)` and a
#           reference definition `[label]: #target` — and every other way of reaching a
#           fragment is refused by (e), because a target the document can reach by a spelling
#           this gate cannot see is a target nothing checks. Links are collected from the whole
#           file, including above the first `## `, so that whether a link is checked does not
#           depend on where in the file it sits. Implicit
#           GitHub heading slugs are REFUSED as link targets, because `#changed-breaking` is
#           not a name — it is "whichever of the six headings spelled that comes first today",
#           and prepending a section changes the answer. An explicit id is a value this file
#           controls; a slug is one GitHub computes.
#
#         · every declared anchor must sit on its own line, one blank line above a heading, and
#           be named `<section-key>-<heading-slug>` — `0.8.0-changed-breaking`. The heading it
#           sits above is identified by the SAME predicate (e) refuses non-canonical headings
#           with, so the two cannot disagree about what a heading is: `##NotAHeading` renders as
#           a paragraph, and an anchor above it names nothing while looking well formed. That
#           name ties
#           the anchor to one heading text in one release section, so a merge that drops the
#           heading orphans the anchor, one that renames it breaks the name, and one that moves
#           it to another section breaks the section key. All three red. This is what closes
#           failure 1 for headings that carry no numbered list.
#
#         · at least one anchor and at least one link must exist — the positive control for (d),
#           so a merge that eats the whole migration table cannot pass (d) vacuously. The
#           per-spelling counts are printed on the OK line, so a green states what each
#           extractor found instead of hiding a zero.
#
#   (e) ONE SPELLING PER CONSTRUCT, and everything else is refused. This is the rule the other
#       five are built on, and it exists because the first two versions of (a)–(d) tried to do
#       the opposite. They enumerated the spellings to accept, and two rounds of adversarial
#       review each turned up more they had missed — `  ### X`, `### X ###`, `1) x`, `[x](<#t>)`,
#       `<a HREF=…>`, `<a ID=…>`, `<a class=… id=…>`, `<a name=…>`, `<div id=…>`, a tag split
#       across two lines, a setext underline, a reference definition with its destination on the
#       next line. Every one of those renders on GitHub — checked against `gh api /markdown`,
#       not assumed — and every one went green. CommonMark has more spellings than anyone will
#       enumerate by hand, so the gate stopped trying: it now names the single form it permits
#       for each construct and refuses the rest, with a message printing the canonical form.
#
#         · a heading is `### Heading text` — hashes at column zero, exactly one space, no
#           trailing whitespace, no closing `#` sequence, no setext underline. (a) keys
#           duplicate detection and (d) keys anchor names off the literal line, so two spellings
#           of one heading are two headings here, and the duplicate goes unseen.
#         · a numbered item is `N. text` at column zero. `N)`, an indented marker and a tab
#           after the marker all render as list items that (b) cannot read.
#         · raw HTML is the anchor declaration and nothing else. `id` and `name` are the
#           complete set of HTML attributes that create a fragment target and `href` the one
#           that follows one — the closed set the spec defines — and GitHub puts `id=` on ANY
#           element into the same `user-content-` namespace as the declaration, so `<div id=…>`
#           competes with it exactly as well as `<a id=…>` does. The attribute is refused where
#           it sits INSIDE a tag, tracked across lines because a tag may span them; naming one
#           in prose is not raw HTML and this changelog documents an HTML-adjacent API, so it
#           has to be able to say `the href="#x" attribute` in a sentence.
#         · a link destination is `#target` bare: no angle brackets, no padding, no title.
#
#       Refusing rather than reading is also the smaller surface. Reading an indented marker
#       would mean deciding, per line, whether it opens a top-level item or a nested one, which
#       takes the whole list-nesting context. This file already keeps every heading and all 46
#       numbered markers at column zero, declares four canonical anchors and writes every one of
#       its 59 links inline, so the rule costs it nothing — measured, before it was written.
#       Fenced code is the one construct read rather than refused: a fence is a fence at any
#       indent CommonMark allows and needs no context to recognize.
#
#   (f) every fenced code block and every HTML comment closes. An unterminated one is a defect
#       in the document before it is a defect in this checker: GitHub swallows everything after
#       a dangling ``` into one grey block, so a `## Section` below it stops being a section for
#       the reader. It is also the way the skip pass could be turned against the checks — what
#       it hides, it hides from (a) through (e) too. Positive control for (e) and (f): at least
#       one line of the file must survive the skip pass and be walked, so a green cannot mean
#       "a region swallowed the document and there was nothing left to check".
#
# Bounds, stated honestly. This checks *structure*, never content: it cannot know an entry is
# wrong, only that two headings collide, that a numbered list restarts, or that a reference
# points at nothing. Heading slugs are computed by an approximation of GitHub's slugger
# (lowercase; drop everything outside [a-z0-9 _-]; spaces to hyphens) which is exact for ASCII
# headings and drops non-ASCII letters GitHub would keep. That approximation is used only to
# derive the *expected* anchor name, never to resolve a link, so an error in it produces a red
# with the name it wants — never a false green. Fenced code blocks (``` / ~~~ indented 0–3, and
# closed only by a run of the same character at least as long) and HTML comments are skipped,
# because neither renders — but only the comment itself: whatever shares a line with one is an
# HTML block that passes through, so `<!-- n --><a href="#t">` is a live link and is scanned,
# while `<!-- n -->### H` is literal text and is not scanned as a heading. An indented code
# block (4 spaces) is not skipped at all, so an in-document link written inside one still has to
# resolve. `12)` is not a numbered marker here, only `12.`. Leading whitespace containing a tab
# counts as column 4 or beyond — which is right at the margin, where a tab means an indented code
# block, and wrong only in the one place a tab could land inside a list item's content and make
# a heading; this file contains no leading tab. Inline code spans and inline comments are masked
# out before the raw-HTML and link scans, but only within one line: a code span that opens on
# one line and closes on the next is masked on neither, so markup inside it is still scanned.
# Written for both mawk (ubuntu-latest) and BSD awk: no interval expressions, no `delete array`,
# no gawk extensions.
#
# KNOWN HOLES, left open on purpose. Three crafted inputs reach a fragment in this document and
# pass. All three were verified against `gh api /markdown`; none is something an ordinary
# changelog edit produces, and each needs a real CommonMark/HTML parser rather than one more
# regex, which is more machinery than a changelog linter earns:
#
#   · `<div data=">" id="hijack"></div>` — the tag scanner ends the tag at the first `>`, and
#     that one is inside a quoted attribute value, so the `id=` after it is never seen. GitHub
#     renders `<div id="user-content-hijack">`. Closing it means tracking quote state inside a
#     tag, which is where regex scanning of HTML stops being honest.
#
#   · `[x](CHANGELOG.md#no-such-anchor)` — a path to this very file plus a fragment reaches the
#     same anchors, but the collector only takes destinations beginning `#`. Recognising it
#     means knowing what this file is called from inside it, and treating `./CHANGELOG.md`,
#     `CHANGELOG.MD` and a URL to the repository as the same document.
#
#   · `[lab]:` with `#no-such-anchor "t"` on the next line — a split reference definition whose
#     continuation carries a title. The split-definition check recognises a bare `#target` on
#     the continuation and not a titled one, so this reads as external and is passed over.
#
# Each is a false GREEN, which is the direction this gate cares about, so they are recorded here
# rather than quietly dropped. What they are not is reachable by accident: every one has to be
# typed on purpose, in a file whose every other spelling is refused by (e).
#
# `ci/changelog_structure_selftest.sh` is the self-test: it mutates this file's own changelog
# one defect at a time and requires every mutation to red, under every awk it can find, with
# byte-identical output. A check nobody has seen fail is not a check. Its part C is a table of
# every CommonMark spelling that reaches a heading, a label, a fragment target or a link, each
# with the disposition owed it — resolved, or refused by name — and it asserts that no spelling
# is silently ignored.
#
# If review turns up yet another rendered spelling that passes: that is evidence (e) failed to
# invert somewhere, not a cue to add a table row. Find the recogniser that is still enumerating
# what to accept. A row added over the top of an un-inverted rule buys one spelling and leaves
# the rest of that family open — which is how this file needed three rounds instead of one.

set -euo pipefail

FILE="${1:-CHANGELOG.md}"

if [ ! -f "$FILE" ]; then
  echo "changelog-structure: $FILE does not exist" >&2
  exit 1
fi

awk -v file="$FILE" '
function fail(msg) { printf("changelog-structure: FAIL — %s\n", msg) > "/dev/stderr"; status = 1 }
function note(msg) { printf("                      %s\n", msg) > "/dev/stderr" }

# An approximation of GitHub'"'"'s heading slugger. Exact for ASCII headings.
function slug(t,   s) {
  s = tolower(t)
  gsub(/[^a-z0-9 _-]/, "", s)
  gsub(/ /, "-", s)
  return s
}

# The stable key for a `## ` section: its first token, lowercased. `0.8.0`, `unreleased`.
function seckey(h,   s) {
  s = h
  sub(/^##[ \t]+/, "", s)
  sub(/[ \t].*$/, "", s)
  s = tolower(s)
  gsub(/[^a-z0-9._-]/, "", s)
  return s
}

function hlevel(s,   n) { n = 0; while (substr(s, n + 1, 1) == "#") n++; return n }
function htext(s,   t)  { t = s; sub(/^#+[ \t]+/, "", t); return t }

# The left indent of a line in columns, capped at 4. A tab advances to the next multiple-of-4
# stop, so a tab anywhere in the leading run already puts the line at column 4 — and 4 is where
# CommonMark stops reading leading space as indentation and starts reading it as code.
function indent(s,   k) {
  k = 0
  while (substr(s, k + 1, 1) == " ") { k++; if (k >= 4) return 4 }
  if (substr(s, k + 1, 1) == "\t") return 4
  return k
}

# The length of the run of `ch` that starts at column `at`.
function runlen(s, at, ch,   k) { k = 0; while (substr(s, at + k + 1, 1) == ch) k++; return k }

# The length of the leading run of ASCII digits.
function digits(s,   k, c) {
  for (k = 0; ; k++) { c = substr(s, k + 1, 1); if (c == "" || index("0123456789", c) == 0) break }
  return k
}

# The level of a CANONICAL ATX heading — hashes at column zero, exactly one space, some text, no
# trailing whitespace, no closing sequence — and 0 for anything else. This is the single
# predicate for "is a heading": (e) refuses every rendered heading it returns 0 for, and (d)
# will not attach an anchor to one. Sharing it is the point. `##NotAHeading` renders as a
# paragraph, so an anchor above it names nothing, and a check that accepted any `##` prefix
# would let that through.
function headlevel(s,   lv, after) {
  if (substr(s, 1, 1) != "#") return 0
  lv = runlen(s, 0, "#")
  if (lv < 1 || lv > 6) return 0
  if (substr(s, lv + 1, 1) != " ") return 0
  after = substr(s, lv + 2, 1)
  if (after == "" || after == " " || after == "\t") return 0
  if (s ~ /[ \t]$/) return 0
  if (s ~ /[ \t]#+$/) return 0
  return lv
}

# Whatever a line leaves outside its HTML comments. Comment content does not render; anything
# on the line beside it does.
function decomment(s,   out, p, q) {
  out = ""
  while ((p = index(s, "<!--")) > 0) {
    out = out substr(s, 1, p - 1)
    s = substr(s, p + 4)
    q = index(s, "-->")
    if (q == 0) return out
    s = substr(s, q + 3)
  }
  return out s
}

# A run of `k` filler characters. Deliberately not spaces: masking must never be able to invent
# trailing whitespace on a heading, or turn a line with content into a blank one.
function fill(k,   t) {
  t = ""
  while (length(t) < k) t = t "xxxxxxxxxxxxxxxx"
  return substr(t, 1, k)
}

# The line with everything blanked that the reader never receives as markup: inline code spans
# and inline HTML comments. GitHub renders `text <!-- <div id="x"></div> --> more` as
# `<p>text  more</p>` and a backticked tag as a code literal — neither declares a fragment nor
# links anywhere, and scanning the raw line called both of them live raw HTML. Code spans are
# taken first, because CommonMark binds them tighter than raw HTML.
#
# Length-preserving, so every column, indent and `ind`-relative substring downstream still
# lines up with the real line. Only the raw-HTML and link scanners read this: heading text is
# read RAW, because (d) derives an anchor name from it and a heading like
# "Known limitation — `finish` does not refuse …" has to keep the slug it actually renders with.
#
# A delimiter with no partner on the line is left alone — unclosed backticks are literal text in
# CommonMark, and an unclosed `<!--` at the start of a line belongs to the block path above.
# That is the conservative direction: it leaves more to be scanned, never less.
function mask(s,   out, p, m2, c, r, j, r2, q) {
  out = ""
  p = 1
  m2 = length(s)
  while (p <= m2) {
    c = substr(s, p, 1)
    if (c == "`") {
      r = runlen(s, p - 1, "`")
      j = p + r
      q = 0
      while (j <= m2) {
        if (substr(s, j, 1) == "`") {
          r2 = runlen(s, j - 1, "`")
          if (r2 == r) { q = j + r2; break }
          j += r2
        } else j++
      }
      if (q) { out = out fill(q - p); p = q; continue }
      out = out substr(s, p, r); p += r; continue
    }
    if (c == "<" && substr(s, p, 4) == "<!--") {
      q = index(substr(s, p), "-->")
      if (q > 0) { out = out fill(q + 2); p += q + 2; continue }
      out = out substr(s, p); break
    }
    out = out c
    p++
  }
  return out
}

# Walk a line tracking whether we are inside a raw HTML tag, and report an `id`, `name` or
# `href` attribute that sits inside one. Two things make this a scan rather than a regex on the
# whole line.
#
# It is keyed on being INSIDE a tag, because this changelog documents an HTML-adjacent API and
# an entry has every right to say `the href="#x" attribute is now handled` in prose. Refusing
# the attribute wherever it appeared made such an entry unmergeable — a false red the gate has
# no business raising. (Inline code already passed, but only by accident: a backtick is not the
# whitespace the old pattern needed in front of the name.)
#
# And `intag` carries ACROSS lines on purpose, because CommonMark lets a tag span them and a
# split tag puts its attributes on the second line — `<div` then `id="x">` is one tag and one
# fragment target. It is cleared at a blank line and at every skipped region, where no tag can
# still be open.
function tagscan(s,   lo, m, k, c, nx, hit) {
  lo = " " tolower(s)
  m = length(lo)
  hit = 0
  for (k = 1; k <= m; k++) {
    c = substr(lo, k, 1)
    if (!intag) {
      if (c == "<") {
        nx = substr(lo, k + 1, 1)
        if (nx != "" && (nx == "/" || index("abcdefghijklmnopqrstuvwxyz", nx) > 0)) intag = 1
      }
      continue
    }
    if (c == ">") { intag = 0; continue }
    if (c == " " || c == "\t") {
      nx = substr(lo, k + 1, 1)
      if (nx == "i" || nx == "n" || nx == "h")
        if (substr(lo, k + 1) ~ /^(id|name|href)[ \t]*=/) hit = 1
    }
  }
  return hit
}

# One diagnosis, whether the HTML sat on a walked line or in the tail of a comment line.
function rawhtml_fail(str, ln) {
  fail("line " ln " carries raw HTML that GitHub turns into a fragment target or a link:")
  note(str)
  note("Canonical: an anchor is `<a id=\"section-key-heading-slug\"></a>` alone on its own")
  note("line, one blank line above the heading it names — and a link is `[text](#target)`.")
  note("`id=` and `name=` on any element land in the same `user-content-` namespace as that")
  note("declaration, so a second spelling silently competes with it for the fragment, and")
  note("the earlier one wins. That is how 53 references came to point at an empty stub.")
}

function addlink(tgt, ln) { nlink++; link_tgt[nlink] = tgt; link_line[nlink] = ln }

{ L[NR] = $0 }

END {
  status = 0
  n = NR

  # ── mark what does not render: fenced code, and HTML comments ──────────────────────────
  # A `### ` inside a fence is not a heading and an `<a id=…>` inside a comment is not an
  # anchor. Neither reaches the reader, so neither may reach the checks. A fence is recognized
  # at any indent CommonMark allows (0–3) and closed only by a run of the same character, at
  # least as long, with nothing but space after it: a `~~~` inside a ``` block is content, and
  # a four-backtick block is not closed by the three backticks inside it.
  infence = 0; incomment = 0; fence_at = 0; comment_at = 0; nregion = 0
  for (i = 1; i <= n; i++) {
    s = L[i]
    ind = indent(s)
    ch = substr(s, ind + 1, 1)
    if (!incomment && ind <= 3 && (ch == "`" || ch == "~")) {
      run = runlen(s, ind, ch)
      if (run >= 3) {
        if (!infence) {
          SKIP[i] = 1; nregion++
          infence = 1; fence_ch = ch; fence_run = run; fence_at = i
          continue
        }
        if (ch == fence_ch && run >= fence_run && substr(s, ind + run + 1) !~ /[^ \t]/) {
          SKIP[i] = 1; infence = 0; fence_at = 0
          continue
        }
      }
    }
    if (infence) { SKIP[i] = 1; continue }
    # A comment line is skipped as MARKDOWN, but whatever sits beside the comment on it is
    # still an HTML block and still passes through to the reader as raw HTML: GitHub renders
    # `<!-- n --><a href="#t">x</a>` as a live link and `<!-- n --><div id="t">` into the
    # `user-content-` namespace, while rendering `<!-- n -->### H` as the literal text `### H`.
    # So the remainder is kept in HTAIL and later scanned as HTML — never as Markdown, which
    # would refuse a heading spelling that does not render as a heading at all.
    if (incomment) {
      SKIP[i] = 1
      if (s ~ /-->/) {
        incomment = 0; comment_at = 0
        HTAIL[i] = decomment(substr(s, index(s, "-->") + 3))
      }
      continue
    }
    if (s ~ /^[ \t]*<!--/) {
      SKIP[i] = 1; nregion++
      if (s ~ /-->/) HTAIL[i] = decomment(s)
      else { incomment = 1; comment_at = i }
    }
  }

  # ── (f) a region that never closes ───────────────────────────────────────────────────────
  if (infence) {
    fail("the code fence opened at line " fence_at " is never closed")
    note("Everything below it renders as one grey block — a `## Section` down there is not a")
    note("section any more, it is literal text — and this gate skips all of it, so nothing in")
    note("it is checked either. Close the fence.")
  }
  if (incomment) {
    fail("the HTML comment opened at line " comment_at " is never closed")
    note("Everything below it is invisible to a reader and skipped by this gate. Close it with")
    note("`-->`.")
  }

  # ── walk: sections, headings, numbered labels, anchors, links ──────────────────────────
  nsec = 0; nanchor = 0; nlink = 0; nscan = 0
  nlink_inline = 0; nlink_ref = 0; prevblank = 1
  for (i = 1; i <= n; i++) {
    if (SKIP[i]) {
      prevblank = 1
      intag = 0
      if (HTAIL[i] ~ /[^ \t]/) {
        if (tagscan(HTAIL[i]) || tolower(HTAIL[i]) ~ /<a[ \t]/ || tolower(HTAIL[i]) ~ /<a$/)
          rawhtml_fail(HTAIL[i], i)
        intag = 0
      }
      continue
    }
    s = L[i]
    ind = indent(s)
    b = substr(s, ind + 1)
    # `v` is the line with inline code spans and inline comments blanked; it is what the
    # raw-HTML and link scanners read. Everything else reads `s`, and diagnostics always echo
    # `s`, so the reader is shown the line as it is written in the file.
    v = mask(s)
    vb = substr(v, ind + 1)
    if (s ~ /[^ \t]/) nscan++; else intag = 0

    # ---- (e) one spelling per construct, and everything else is refused -------------------
    # Two review rounds each found another rendered spelling the recognisers did not match, so
    # the recognisers stopped enumerating what to accept. Each construct below has exactly one
    # permitted spelling; anything else CommonMark would render as the same construct is
    # refused by name. Every refusal prints the canonical form it wanted.

    # headings: `### Text` — hashes at column zero, one space, nothing trailing.
    if (ind <= 3 && substr(b, 1, 1) == "#") {
      lvl = hlevel(b)
      nx = substr(b, lvl + 1, 1)
      if (lvl <= 6 && (nx == "" || nx == " " || nx == "\t") && headlevel(s) == 0) {
        why = ""
        if (ind > 0)                            why = "it is indented " ind " space(s)"
        else if (nx == "\t")                    why = "a tab follows the hashes, not one space"
        else if (nx == "")                      why = "it has no heading text"
        else if (substr(b, lvl + 2, 1) == " ")  why = "more than one space follows the hashes"
        else if (s ~ /[ \t]$/)                  why = "it has trailing whitespace"
        else if (s ~ /[ \t]#+$/)                why = "it carries a closing `#` sequence"
        if (why != "") {
          fail("line " i " is a heading GitHub renders, spelled a way this file does not — " why ":")
          note(s)
          note("Canonical: `### Heading text` — hashes at column zero, exactly one space, no")
          note("trailing whitespace, no closing hashes. GitHub renders `  ## X`, `## X ##` and")
          note("`## X ` as one and the same heading, while (a) keys duplicate detection and (d)")
          note("keys every anchor name off the literal line — so a second spelling of a heading")
          note("is a second heading here, and the duplicate goes unseen.")
        }
      }
    }

    # headings: a setext underline makes a heading out of the line above it.
    if (ind <= 3 && !prevblank && (b ~ /^=+[ \t]*$/ || b ~ /^-+[ \t]*$/)) {
      fail("line " i " is a setext heading underline — it makes line " (i - 1) " a heading:")
      note(s)
      note("Canonical: `### Heading text`. Every rule here reads the line above as prose, so")
      note("that heading is invisible to (a) and (d). If you meant a thematic break, leave a")
      note("blank line above it.")
    }

    # ordered markers: `N. text` — digits, a period, one space, at column zero.
    if (ind <= 3) {
      d = digits(b)
      if (d > 0) {
        dl = substr(b, d + 1, 1)
        af = substr(b, d + 2, 1)
        if ((dl == "." || dl == ")") && (af == "" || af == " " || af == "\t") &&
            !(ind == 0 && dl == "." && af == " ")) {
          fail("line " i " is a numbered list item GitHub renders, spelled a way this file does not:")
          note(s)
          note("Canonical: `N. text` — the digits, a period, one space, at column zero. `N)`, an")
          note("indented marker, a tab after the marker and a marker with nothing after it all")
          note("render as list items that (b) does not read, so a duplicated label rides in")
          note("unseen — which is the accident that ate a heading and filed six items under the")
          note("wrong one. Nest with a bullet sub-list; it needs no numbering to stay unambiguous.")
        }
      }
    }

    # ---- raw HTML: the canonical declaration, and nothing else ---------------------------
    # `id` and `name` are the complete set of HTML attributes that create a fragment target,
    # and `href` the one that follows a fragment — the closed set the spec defines, not a list
    # of tag spellings someone thought of. Keyed on the attribute rather than on `<a …>`
    # because GitHub puts `id=` on ANY element into the same `user-content-` namespace as the
    # declaration below: `<div id="x">` and `<a name="x">` compete with it just as well.
    # Verified against `gh api /markdown`, not assumed. Runs BEFORE the declaration branch so
    # that `intag` is advanced by every rendered line, declaration included.
    attrhit = tagscan(v)
    lo = tolower(v)
    if (s !~ /^<a id="[^"]*"><\/a>[ \t]*$/ &&
        (attrhit || lo ~ /<a[ \t]/ || lo ~ /<a$/))
      rawhtml_fail(s, i)

    # ---- explicit anchor declaration ----------------------------------------------------
    if (s ~ /^<a id="[^"]*"><\/a>[ \t]*$/) {
      id = s
      sub(/^<a id="/, "", id)
      sub(/"><\/a>[ \t]*$/, "", id)
      nanchor++
      anchor_id[nanchor] = id
      anchor_line[nanchor] = i
      anchor_sec[nanchor] = nsec
      if (id_line[id]) fail("anchor id `" id "` is declared twice (lines " id_line[id] " and " i ")")
      else id_line[id] = i
      prevblank = 0
      continue
    }

    # ---- in-document links: one spelling inline, one as a reference definition -------------
    # Collected before the section branch takes its `continue`, so that whether a link is
    # checked never depends on where in the file it sits. A destination that reaches into this
    # document in any other spelling is refused, not guessed at: GitHub follows `](<#t>)`,
    # `]( #t )` and `](#t "title")` to the same fragment, and a resolver that matches only one
    # of them lets the other two through unchecked.
    rest = v
    while (match(rest, /\]\([^)]*\)/)) {
      raw = substr(rest, RSTART + 2, RLENGTH - 3)
      rest = substr(rest, RSTART + RLENGTH)
      t = raw
      sub(/^[ \t]+/, "", t); sub(/[ \t]+$/, "", t); sub(/^</, "", t); sub(/>$/, "", t)
      if (substr(t, 1, 1) != "#") continue
      if (raw ~ /^#[^ \t"<>()]*$/) { addlink(substr(raw, 2), i); nlink_inline++ }
      else {
        fail("line " i " links into this document in a spelling this file does not use:")
        note("](" raw ")")
        note("Canonical: `[text](#target)` — no angle brackets, no padding, no title. GitHub")
        note("follows every one of those to the same fragment; only this one is resolved here.")
      }
    }
    if (ind <= 3 && vb ~ /^\[[^]]*\][ \t]*:/) {
      rd = vb
      sub(/^\[[^]]*\][ \t]*:/, "", rd)
      sub(/^[ \t]+/, "", rd); sub(/[ \t]+$/, "", rd)
      if (rd == "") {
        # CommonMark reads the NEXT line as the destination. Only an in-document destination
        # concerns this gate: `[docs]:` over `https://example.com` is a perfectly good external
        # reference, and refusing it was an over-reach of the refuse-by-default posture.
        cont = mask(L[i + 1])
        sub(/^[ \t]+/, "", cont); sub(/[ \t]+$/, "", cont)
        sub(/^</, "", cont); sub(/>$/, "", cont)
        if (cont ~ /^#[^ \t]*$/) {
          fail("line " i " is a link reference definition whose destination reaches into this")
          note("document from the next line, where this gate does not look for it:")
          note(s)
          note("Canonical: `[label]: #target`, destination on the same line.")
        }
      } else {
        t = rd
        sub(/^</, "", t); sub(/>$/, "", t)
        if (substr(t, 1, 1) == "#") {
          if (rd ~ /^#[^ \t"<>()]*$/) { addlink(substr(rd, 2), i); nlink_ref++ }
          else {
            fail("line " i " defines a reference into this document in a spelling this file does not use:")
            note(s)
            note("Canonical: `[label]: #target` — no angle brackets, no title.")
          }
        }
      }
    }

    # ---- `## ` opens a new section -----------------------------------------------------
    if (s ~ /^## /) {
      nsec++
      sec_name[nsec] = s
      sec_line[nsec] = i
      sec_key[nsec]  = seckey(s)
      if (sec_key[nsec] == "")
        fail("section heading at line " i " has no usable version token: " s)
      if (keyseen[sec_key[nsec]]++)
        fail("two `## ` sections share the key `" sec_key[nsec] "` (line " i "): " s)
      prevblank = 0
      continue
    }

    if (nsec == 0) { prevblank = (s !~ /[^ \t]/); continue }

    if (s ~ /[^ \t]/) sec_content[nsec]++

    # ---- headings ------------------------------------------------------------------------
    if (s ~ /^###/) {
      lvl = hlevel(s)
      if (lvl >= 3 && lvl <= 6 && substr(s, lvl + 1, 1) == " ") {
        if (lvl == 3) sec_h3[nsec]++
        hkey = nsec SUBSEP s
        if (hline[hkey]) {
          fail("duplicate heading in `" sec_name[nsec] "` (lines " hline[hkey] " and " i "):")
          note(s)
          note("Merge them. A reader cannot tell which of two same-named sections an item is")
          note("in, and a numbered cross-reference into either one is ambiguous.")
        } else hline[hkey] = i
      }
    }

    # ---- numbered-list labels --------------------------------------------------------------
    if (s ~ /^[0-9]+\. /) {
      lab = s
      sub(/\..*$/, "", lab)
      sec_nnum[nsec]++
      lkey = nsec SUBSEP lab
      if (lline[lkey]) {
        fail("repeated numbered label " lab " in `" sec_name[nsec] "` (lines " lline[lkey] " and " i ")")
        note("Renumber once across the section. A restarting list makes `breaking change 3`")
        note("ambiguous, which is exactly what a migration guide cannot afford.")
      } else lline[lkey] = i
    }

    prevblank = (s !~ /[^ \t]/)
  }

  # ── (c) positive control ────────────────────────────────────────────────────────────────
  if (nsec == 0) {
    fail("no `## ` release heading found in " file)
    exit_now = 1
  }
  for (k = 1; k <= nsec && !exit_now; k++) {
    if (!sec_content[k]) fail("the `" sec_name[k] "` section (line " sec_line[k] ") is empty")
    if (!sec_h3[k])      fail("`" sec_name[k] "` (line " sec_line[k] ") carries no `### ` heading")
  }

  # ── (e)/(f) positive control ────────────────────────────────────────────────────────────
  if (nscan == 0)
    fail("not one line of " file " survives the skip pass — a fence or a comment swallowed the whole file, and every check above passed on nothing")

  # ── (d) anchor placement and naming ─────────────────────────────────────────────────────
  for (a = 1; a <= nanchor; a++) {
    i = anchor_line[a]
    id = anchor_id[a]

    if (L[i + 1] ~ /[^ \t]/) {
      fail("anchor `" id "` (line " i ") is not followed by a blank line. Leave one: it is")
      note("correct under both readings of a lone tag — inline HTML in a paragraph, which is")
      note("what CommonMark says, and an HTML block, which would swallow the heading below —")
      note("and it gives this check one deterministic place to look for the heading.")
      continue
    }
    h = L[i + 2]
    # `headlevel` and not a `^##` prefix: `##NotAHeading` is a paragraph, GitHub wants the
    # space, so an anchor above it names nothing that exists. Same predicate the walk refuses
    # non-canonical headings with, so the two can never disagree about what a heading is.
    if (SKIP[i + 2] || headlevel(h) < 2) {
      fail("anchor `" id "` (line " i ") does not sit above a heading — line " (i + 2) " is:")
      note(h == "" ? "(blank)" : h)
      note("An orphaned anchor means its heading was renamed, moved or eaten. That is the")
      note("exact accident this gate exists for: fix the heading, do not delete the anchor.")
      note("`##NotAHeading` does not count: without the space GitHub renders a paragraph, and")
      note("the anchor would point at nothing while looking perfectly well formed.")
      continue
    }
    key = anchor_sec[a] ? sec_key[anchor_sec[a]] : ""
    if (headlevel(h) == 2) want = seckey(h)
    else                   want = key "-" slug(htext(h))
    if (id != want) {
      fail("anchor `" id "` (line " i ") does not name the heading it sits above.")
      note("expected: <a id=\"" want "\"></a>")
      note("The name is `<section-key>-<heading-slug>` so that renaming the heading, or moving")
      note("it to another release section, cannot leave a live link pointing at the old place.")
    }
  }

  # ── (d) link resolution ─────────────────────────────────────────────────────────────────
  # Grouped by target: 53 copies of one diagnosis is not 53 findings, and a wall of repeats is
  # a log nobody reads to the end.
  nbad = 0
  for (l = 1; l <= nlink; l++) {
    tgt = link_tgt[l]
    if (id_line[tgt]) continue
    if (!(tgt in bad_n)) { nbad++; bad_order[nbad] = tgt; bad_at[tgt] = "" }
    bad_n[tgt]++
    if (bad_n[tgt] <= 8) bad_at[tgt] = bad_at[tgt] " " link_line[l]
    else if (bad_n[tgt] == 9) bad_at[tgt] = bad_at[tgt] " …"
  }
  for (b = 1; b <= nbad; b++) {
    tgt = bad_order[b]
    fail("link to `#" tgt "` resolves to no declared anchor — " bad_n[tgt] " occurrence(s), lines" bad_at[tgt])
    if (tgt ~ /-[0-9]+$/) {
      note("A `-N` suffix is GitHub'"'"'s duplicate-slug counter — pure document order. It is never")
      note("a valid target here: inserting a section renumbers it.")
    }
    note("Declare `<a id=\"" tgt "\"></a>` one blank line above the heading it means, or point")
    note("the link at an anchor that exists. An implicit heading slug is not a link target in")
    note("this file: it names whichever same-spelled heading a future merge happens to put")
    note("first, which is how 53 references into `0.8.0` came to point at an empty stub.")
  }

  # ── (d) positive control ────────────────────────────────────────────────────────────────
  if (nanchor == 0) fail("no `<a id=\"…\"></a>` anchor declared in " file " — (d) would pass vacuously")
  if (nlink == 0)   fail("no in-document link (`](#…)` or `[label]: #…`) found in " file " — (d) would pass vacuously")

  if (status == 0) {
    printf "changelog-structure: OK — %d section(s), %d anchor(s), %d in-document link(s), all resolved.\n", nsec, nanchor, nlink
    printf "  links: %d inline, %d reference-style · %d skipped region(s) · %d line(s) walked\n", nlink_inline + 0, nlink_ref + 0, nregion + 0, nscan + 0
    for (k = 1; k <= nsec; k++)
      printf "  %-24s %3d heading(s), %3d numbered item(s)\n", sec_key[k], sec_h3[k] + 0, sec_nnum[k] + 0
  }
  exit status
}
' "$FILE"
