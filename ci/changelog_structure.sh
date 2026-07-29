#!/bin/bash
#
# Structural check for the changelog's newest release section.
#
# The changelog is a load-bearing release artifact that no compiler, linter or test reads.
# It has already failed in production once: a rebase resolved `CHANGELOG.md` as a verbatim
# union — textually correct, no conflict markers — and silently dropped a `### Changed
# (breaking)` heading, filing six breaking changes under `### Performance`. Every other gate
# passed, because nothing reads a changelog.
#
# This checks *structure*, never content. It cannot know an entry is wrong, only that two
# headings collide or that a numbered list restarts. That is the honest bound, and it is
# still worth the second it costs.
#
# Checks, over the newest `## <version-or-Unreleased>` section only:
#   (a) no duplicate `### ` heading;
#   (b) numbered-list labels are unique within the section — no two `1.`, no repeats;
#   (c) the section is non-empty and carries at least one `### ` heading.
#
# (c) is the positive control: without it a green result could mean "the section-extraction
# matched nothing" rather than "the section is well formed". A check that cannot fail is not
# a check.

set -euo pipefail

FILE="${1:-CHANGELOG.md}"

if [ ! -f "$FILE" ]; then
  echo "changelog-structure: $FILE does not exist" >&2
  exit 1
fi

# The newest section: from the first `## ` line to the line before the next one.
SECTION=$(awk '
  /^## / { if (seen) exit; seen = 1; header = $0; next }
  seen   { print }
' "$FILE")

SECTION_HEADER=$(awk '/^## /{ print; exit }' "$FILE")

status=0

# ── (c) positive control, first: a green run must mean the extraction worked ──────────────
if [ -z "${SECTION_HEADER}" ]; then
  echo "changelog-structure: FAIL — no '## ' release heading found in $FILE" >&2
  exit 1
fi

if [ -z "$(printf '%s' "$SECTION" | tr -d '[:space:]')" ]; then
  echo "changelog-structure: FAIL — the '${SECTION_HEADER}' section is empty" >&2
  status=1
fi

heading_count=$(printf '%s\n' "$SECTION" | grep -c '^### ' || true)
if [ "$heading_count" -eq 0 ]; then
  echo "changelog-structure: FAIL — '${SECTION_HEADER}' carries no '### ' heading" >&2
  status=1
fi

# ── (a) no duplicate `### ` heading ──────────────────────────────────────────────────────
dupes=$(printf '%s\n' "$SECTION" | grep '^### ' | sort | uniq -d || true)
if [ -n "$dupes" ]; then
  echo "changelog-structure: FAIL — duplicate '### ' headings in '${SECTION_HEADER}':" >&2
  printf '%s\n' "$dupes" >&2
  echo "  Merge them. A reader cannot tell which of two same-named sections an item is in," >&2
  echo "  and a numbered cross-reference into either one is ambiguous." >&2
  status=1
fi

# ── (b) numbered-list labels unique within the section ───────────────────────────────────
label_dupes=$(printf '%s\n' "$SECTION" \
  | grep -E '^[0-9]+\. ' \
  | sed -E 's/^([0-9]+)\..*/\1/' \
  | sort -n | uniq -d || true)
if [ -n "$label_dupes" ]; then
  echo "changelog-structure: FAIL — repeated numbered-list labels in '${SECTION_HEADER}':" >&2
  printf '  item %s appears more than once\n' $label_dupes >&2
  echo "  Renumber once across the section. A restarting list makes 'breaking change 3'" >&2
  echo "  ambiguous, which is exactly what a migration guide cannot afford." >&2
  status=1
fi

if [ "$status" -eq 0 ]; then
  numbered=$(printf '%s\n' "$SECTION" | grep -cE '^[0-9]+\. ' || true)
  echo "changelog-structure: OK — '${SECTION_HEADER}': ${heading_count} heading(s), ${numbered} numbered item(s), no collisions."
fi

exit "$status"
