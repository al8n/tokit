#!/usr/bin/env python3
"""Validate Tokora's guide source and generated mdBook output with the standard library."""

from __future__ import annotations

import argparse
import html
import re
import sys
import tomllib
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote


ROOT = Path(__file__).resolve().parents[2]
GUIDE = ROOT / "tokora" / "src" / "guide"
README = ROOT / "README.md"
MANIFEST = ROOT / "tokora" / "Cargo.toml"

CHAPTERS = (
    ("ch01_tokens.md", "1. Tokens and the lexer"),
    ("ch02_parsers.md", "2. First parsers"),
    ("ch03_combinators.md", "3. Composition"),
    ("ch04_dispatch.md", "4. Deterministic choice"),
    ("ch05_pratt.md", "5. Expressions: Pratt parsing"),
    ("ch06_backtracking.md", "6. Backtracking"),
    ("ch07_diagnostics.md", "7. Diagnostics"),
    ("ch08_recovery.md", "8. Recovery"),
    ("ch09_streaming.md", "9. Partial input"),
    ("ch10_testing.md", "10. Testing"),
    ("arch_parsing_engine.md", "The parsing engine: parse while lexing"),
    ("arch_checkpoint_rewind.md", "Checkpoint, rewind, and the LIFO contract"),
    ("arch_atomic_emitter.md", "The atomic emitter"),
    ("arch_event_stream_cst.md", "The event-stream CST engine"),
    ("arch_source_slice.md", "Source, Slice, and storage backends"),
    ("ch11_real_parser.md", "11. Anatomy of a real Tokora parser"),
    ("recipe_custom_lexer.md", "Recipe: writing a custom lexer"),
    ("ch12_calculator_example.md", "12. Walkthrough: calculator"),
    ("ch13_s_expression_example.md", "13. Walkthrough: S-expressions"),
    ("ch14_json_example.md", "14. Walkthrough: JSON"),
    ("ch15_c_expression_example.md", "15. Walkthrough: C expressions"),
    ("ch16_lossless_cst.md", "16. Lossless CSTs with Rowan"),
    ("ref_combinators.md", "Reference: combinators & atoms"),
    ("ref_errors_emitters_context.md", "Reference: errors, emitters & context"),
    ("ref_vocabulary_macros_features.md", "Reference: vocabulary, macros & feature flags"),
    ("ref_pratt.md", "Reference: Pratt (precedence) parsing"),
    ("ref_types_syntax.md", "Reference: types & syntax building blocks"),
)

SUMMARY = """# Summary

# Part I — Getting Started

- [Introduction](README.md)

# Part II — Core Concepts

- [Tokens and the lexer](ch01_tokens.md)
- [First parsers](ch02_parsers.md)
- [Composition](ch03_combinators.md)
- [Deterministic choice](ch04_dispatch.md)
- [Expressions: Pratt parsing](ch05_pratt.md)
- [Backtracking](ch06_backtracking.md)
- [Diagnostics](ch07_diagnostics.md)
- [Recovery](ch08_recovery.md)
- [Partial input](ch09_streaming.md)
- [Testing](ch10_testing.md)

# Part III — Design & Architecture

- [The parsing engine: parse while lexing](arch_parsing_engine.md)
- [Checkpoint, rewind & the LIFO contract](arch_checkpoint_rewind.md)
- [The atomic emitter](arch_atomic_emitter.md)
- [The event-stream CST engine](arch_event_stream_cst.md)
- [Source, Slice & storage backends](arch_source_slice.md)

# Part IV — Recipes & Applied Parsers

- [Anatomy of a real Tokora parser](ch11_real_parser.md)
- [Recipe: writing a custom lexer](recipe_custom_lexer.md)
- [Walkthrough: calculator](ch12_calculator_example.md)
- [Walkthrough: S-expressions](ch13_s_expression_example.md)
- [Walkthrough: JSON](ch14_json_example.md)
- [Walkthrough: C expressions](ch15_c_expression_example.md)
- [Lossless CSTs with Rowan](ch16_lossless_cst.md)

# Part V — Reference

- [Combinator & atom reference](ref_combinators.md)
- [Errors, emitters & context reference](ref_errors_emitters_context.md)
- [Vocabulary, macros & feature flags](ref_vocabulary_macros_features.md)
- [Pratt (precedence) reference](ref_pratt.md)
- [Types & syntax building blocks](ref_types_syntax.md)
"""

SECTIONS = {
    "arch_parsing_engine.md": (
        "Lex, then parse — and why tokora does neither in that order",
        "Two objects: the input owner and the working handle",
        "A parser is a function over the handle", "The engine, end to end",
    ),
    "arch_checkpoint_rewind.md": (
        "What a checkpoint captures",
        "The last-in, first-out contract, and how misuse is caught",
        "The rewind, end to end",
    ),
    "arch_atomic_emitter.md": (
        "checkpoint / rewind / release: the emitter's transactional surface",
        "The built-ins as design points", "One assembly, two effect channels",
    ),
    "arch_event_stream_cst.md": (
        "Events, not eager nodes", "The event vocabulary", "The rewindable sink",
        "Materialization: one walk that builds and validates",
    ),
    "arch_source_slice.md": (
        "The seam is two traits", "The backends", "no_std posture",
    ),
    "ch11_real_parser.md": (
        "Start with the output", "Build the lexical layer", "Choose a parser shape",
        "Wire the entry point", "Test the complete program", "Map the maintained examples",
    ),
    "recipe_custom_lexer.md": (
        "Step 1 — the token vocabulary", "Step 4 — the hand-written path",
        "Step 5 — trivia and losslessness",
    ),
    "ch12_calculator_example.md": (
        "Define token, kind, lexer alias, and CalcError",
        "Define the precedence constants and grouping sentinel",
        "Implement try_pratt_lhs and try_pratt_rhs",
        "Implement the named prefix, infix, and postfix folds",
        "Build calc_expr", "Reproduce the maintained assertion table",
    ),
    "ch13_s_expression_example.md": (
        "Define tokens and the AST/value types",
        "Implement atom and built-in branches in parse_expr",
        "Implement quote and parenthesized branches",
        "Implement parse_list, including the closing parenthesis",
        "Implement eval and apply", "Exercise the maintained forms",
    ),
    "ch14_json_example.md": (
        "Define borrowed tokens, JsonError, punctuator mappings, and JsonValue",
        "Build boolean, null, number, and string",
        "Build arrays with tentative values, comma separation, delimiters, and collection",
        "Build fields and objects with separated_by_comma_while",
        "Implement tentative try_json_value",
        "Implement committed json_value with an expected-kind diagnostic",
        "Parse sample.json and test separators",
    ),
    "ch15_c_expression_example.md": (
        "Define the lexer, token kinds, and CExprError",
        "Define UnaryOp, BinOp, PostfixOp, and Expr", "Define the precedence ladder",
        "Implement parse_lhs and parse_rhs", "Implement the three folds",
        "Close the recursion in parse_cexpr", "Reproduce the maintained assertion table",
    ),
    "ch16_lossless_cst.md": (
        "Enable Rowan", "One enum owns the kind space", "The grammar declares the tree",
        "Tokens reach the tree on their own", "Backtracking rewinds the tree",
        "Materialization is a typed wall",
    ),
    "ref_combinators.md": (
        "Atoms — parsers from nothing", "Repetition & folding",
        "Separation — comma-separated and friends", "Delimited shapes", "Feature matrix",
    ),
    "ref_errors_emitters_context.md": (
        "The error model", "Emitters", "ParseContext / ComposableParseContext",
    ),
    "ref_pratt.md": (
        "Folds are fn items, not closures", "Token-level surface", "AST-level surface",
    ),
    "ref_types_syntax.md": (
        "Span, offset & location primitives", "Literals",
        "Error recovery: ErrorNode and Recoverable",
    ),
    "ref_vocabulary_macros_features.md": (
        "The punctuator! macro", "The keyword! macro & KeywordToken", "Feature matrix",
    ),
}

EXAMPLES = (
    ("ch12_calculator_example.md", "tokora/examples/calculator.rs", ("PrattToken", "calc_expr")),
    ("ch13_s_expression_example.md", "tokora/examples/s_expression.rs", ("parse_expr", "parse_list", "eval")),
    ("ch14_json_example.md", "tokora/examples/json.rs", ("try_json_value", "json_value", "list", "object")),
    ("ch15_c_expression_example.md", "tokora/examples/c_expression.rs", ("parse_lhs", "parse_rhs", "fold_postfix", "parse_cexpr")),
)


def add(errors: list[str], message: str) -> None:
    errors.append(message)


def visible_lines(path: Path) -> list[tuple[int, str]]:
    result: list[tuple[int, str]] = []
    active: str | None = None
    fence_pattern = re.compile(r"^\s*(" + chr(96) + r"{3,}|~{3,})")
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        marker = fence_pattern.match(line)
        if marker:
            marker_char = marker.group(1)[0]
            active = marker_char if active is None else None if active == marker_char else active
            result.append((number, line))
        elif active is None:
            result.append((number, line))
    return result


def clean(value: str) -> str:
    value = value.replace(chr(96), "")
    value = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", value)
    return html.unescape(re.sub(r"<[^>]+>", "", value)).strip()


def headings(path: Path) -> list[tuple[int, int, str]]:
    result: list[tuple[int, int, str]] = []
    for number, line in visible_lines(path):
        match = re.match(r"^(#{1,6})\s+(.+?)\s*#*\s*$", line)
        if match:
            result.append((number, len(match.group(1)), clean(match.group(2))))
    return result


def slug(value: str) -> str:
    value = re.sub(r"[^\w\s-]", "", clean(value).lower())
    return re.sub(r"[\s-]+", "-", value).strip("-")


def anchor_set(path: Path) -> set[str]:
    counts: dict[str, int] = {}
    result: set[str] = set()
    for _line, _level, heading in headings(path):
        base = slug(heading)
        count = counts.get(base, 0)
        counts[base] = count + 1
        result.add(base if count == 0 else f"{base}-{count}")
    return result


def validate_summary(errors: list[str]) -> None:
    path = GUIDE / "SUMMARY.md"
    actual = path.read_text(encoding="utf-8")
    if actual != SUMMARY:
        add(errors, "tokora/src/guide/SUMMARY.md does not match the approved structure exactly")
    names = re.findall(r"^\s*-\s+\[[^\]]+\]\(((?:ch\d{2}|arch|ref|recipe)_[^)]+\.md)\)\s*$", actual, re.M)
    expected = [name for name, _title in CHAPTERS]
    if names != expected or len(names) != len(set(names)):
        add(errors, "SUMMARY.md must contain the ordered, unique 27 chapter links")
    found = sorted(
        path.name
        for pattern in ("ch[0-9][0-9]_*.md", "arch_*.md", "recipe_*.md", "ref_*.md")
        for path in GUIDE.glob(pattern)
    )
    if found != sorted(expected):
        add(errors, "tokora/src/guide must contain exactly the 27 chapter files in SUMMARY.md")


def validate_headings(errors: list[str]) -> None:
    intro = [text for _line, level, text in headings(GUIDE / "README.md") if level == 1]
    if intro != ["The Tokora Guide"]:
        add(errors, "tokora/src/guide/README.md must have one visible H1: The Tokora Guide")
    for filename, title in CHAPTERS:
        path = GUIDE / filename
        first_level = [text for _line, level, text in headings(path) if level == 1]
        if first_level != [title]:
            add(errors, f"{path.relative_to(ROOT)} must have one visible H1: {title}")
        observed = {text for _line, _level, text in headings(path)}
        for section in SECTIONS.get(filename, ()):
            if section not in observed:
                add(errors, f"{path.relative_to(ROOT)} is missing required section: {section}")


def validate_links(errors: list[str]) -> None:
    link = re.compile(r"(?<!\!)\[[^\]]*\]\(([^)\s]+)\)")
    for source in sorted(GUIDE.glob("*.md")):
        for target in link.findall(source.read_text(encoding="utf-8")):
            if target.startswith(("http://", "https://", "mailto:", "crate::", "super::", "/")):
                continue
            file_part, marker, fragment = target.partition("#")
            if marker and not file_part:
                destination = source
            elif file_part.endswith(".md"):
                destination = (source.parent / unquote(file_part)).resolve()
            else:
                continue
            if not destination.is_file():
                add(errors, f"{source.relative_to(ROOT)} links to missing local Markdown file {target}")
            elif marker and unquote(fragment) not in anchor_set(destination):
                add(errors, f"{source.relative_to(ROOT)} links to missing local fragment {target}")


def example_url(relative: str) -> str:
    return f"https://github.com/al8n/tokora/blob/main/{relative}"


def validate_examples(errors: list[str]) -> None:
    map_text = (GUIDE / "ch11_real_parser.md").read_text(encoding="utf-8")
    for chapter, relative, symbols in EXAMPLES:
        source = ROOT / relative
        page_text = (GUIDE / chapter).read_text(encoding="utf-8")
        url = example_url(relative)
        if not source.is_file():
            add(errors, f"canonical example is missing: {relative}")
            continue
        source_text = source.read_text(encoding="utf-8")
        for text, label in ((map_text, "ch11_real_parser.md"), (page_text, chapter)):
            if url not in text:
                add(errors, f"{label} lacks canonical link {url}")
            for symbol in symbols:
                if not re.search(rf"\b{re.escape(symbol)}\b", text):
                    add(errors, f"{label} lacks named symbol {symbol}")
        for symbol in symbols:
            if not re.search(rf"\b{re.escape(symbol)}\b", source_text):
                add(errors, f"{relative} lacks expected symbol {symbol}")


def feature_section(label: str, path: Path, heading: str) -> tuple[str, set[str]] | None:
    """The backticked feature keys named in the first column of `heading`'s tables.

    Every backticked token in the first cell counts, not just a cell that is *only* a
    backticked token: the guide spells rows as `` `std` *(default)* ``,
    `` `logos` (`= logos_0_16`) `` and `` `bytes` / `bstr` / ... ``, and a reader takes each
    of those as a named feature. A cell with no backticks — a header, a rule, the
    `*(neither)*` tier — names nothing and contributes nothing.
    """
    section = re.search(r"^" + re.escape(heading) + r"\s*$([\s\S]*?)(?=^## |\Z)",
                        path.read_text(encoding="utf-8"), re.M)
    if section is None:
        add(errors_sink, f"{label} must include a {heading!r} section")
        return None
    tick = chr(96)
    keys: set[str] = set()
    for line in section.group(1).splitlines():
        if not line.startswith("|") or line.count("|") < 2:
            continue
        keys.update(re.findall(tick + r"([^" + tick + r"]+)" + tick, line.split("|")[1]))
    return section.group(1), keys


# `validate_features` reports through this rather than threading `errors` into the reader,
# so a missing section is one message from one place.
errors_sink: list[str] = []


def validate_features(errors: list[str]) -> None:
    """Check the three feature tables against `Cargo.toml`, each at the strictness it claims.

    * `README.md`'s **## Features** and the vocabulary chapter's **## Feature matrix** are
      exhaustive — the latter says so in its own prose ("this is the full table, read from
      `Cargo.toml`") — so both are checked for equality.
    * `ref_combinators.md`'s **## Feature matrix** is the summary the vocabulary chapter calls
      it, and making it exhaustive would put a second copy of the backend and tooling rows in
      the combinator reference. It is checked for *no unknown keys*, plus the gates gating what
      that chapter documents: `combinators` and the families it covers. Those thirteen are read
      out of the manifest rather than listed here, so adding a family reds this until the
      chapter names its gate.
    """
    manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))["features"]
    expected = set(manifest)
    families = {"combinators", *manifest["combinators"]}

    errors_sink.clear()
    exhaustive = (
        ("README.md", README, "## Features"),
        ("tokora/src/guide/ref_vocabulary_macros_features.md",
         GUIDE / "ref_vocabulary_macros_features.md", "## Feature matrix"),
    )
    for label, path, heading in exhaustive:
        read = feature_section(label, path, heading)
        if read is None:
            continue
        _text, rows = read
        missing, extra = sorted(expected - rows), sorted(rows - expected)
        if missing:
            add(errors, f"{label} {heading!r} is missing: " + ", ".join(missing))
        if extra:
            add(errors, f"{label} {heading!r} has unknown keys: " + ", ".join(extra))

    label = "tokora/src/guide/ref_combinators.md"
    read = feature_section(label, GUIDE / "ref_combinators.md", "## Feature matrix")
    if read is not None:
        _text, rows = read
        extra, absent = sorted(rows - expected), sorted(families - rows)
        if extra:
            add(errors, f"{label} '## Feature matrix' has unknown keys: " + ", ".join(extra))
        if absent:
            add(errors, f"{label} '## Feature matrix' is missing the combinator gates it "
                        "documents: " + ", ".join(absent))
    errors.extend(errors_sink)


class ContentHeadings(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.headings: list[str] = []
        self._main_depth = 0
        self._heading: list[str] | None = None

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag == "main":
            self._main_depth += 1
        elif tag == "h1" and self._main_depth:
            self._heading = []

    def handle_data(self, data: str) -> None:
        if self._heading is not None:
            self._heading.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag == "h1" and self._heading is not None:
            self.headings.append(" ".join(" ".join(self._heading).split()))
            self._heading = None
        elif tag == "main" and self._main_depth:
            self._main_depth -= 1


def content_h1s(path: Path) -> list[str]:
    parser = ContentHeadings()
    parser.feed(path.read_text(encoding="utf-8"))
    parser.close()
    return parser.headings


class SidebarToc(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.chapter_hrefs: list[str] = []
        self.part_titles: list[str] = []
        self._chapter_list_depth = 0
        self._part_title: list[str] | None = None

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attributes = dict(attrs)
        classes = set((attributes.get("class") or "").split())
        if tag == "ol" and "chapter" in classes:
            self._chapter_list_depth += 1
        elif tag == "li" and self._chapter_list_depth and "part-title" in classes:
            self._part_title = []
        elif tag == "a" and self._chapter_list_depth:
            href = attributes.get("href")
            if href is not None:
                self.chapter_hrefs.append(href)

    def handle_data(self, data: str) -> None:
        if self._part_title is not None:
            self._part_title.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag == "li" and self._part_title is not None:
            self.part_titles.append(" ".join(" ".join(self._part_title).split()))
            self._part_title = None
        elif tag == "ol" and self._chapter_list_depth:
            self._chapter_list_depth -= 1


def sidebar_toc(path: Path) -> SidebarToc:
    parser = SidebarToc()
    parser.feed(path.read_text(encoding="utf-8"))
    parser.close()
    return parser


def validate_book(errors: list[str], book: Path) -> None:
    if not book.is_dir():
        add(errors, f"mdBook output directory does not exist: {book}")
        return
    for filename, title in CHAPTERS:
        page = book / filename.replace(".md", ".html")
        if not page.is_file() or page.stat().st_size == 0:
            add(errors, f"generated guide page is missing: {page}")
        elif content_h1s(page) != [title]:
            add(errors, f"generated guide page is missing its content H1: {page.name}")
    index, not_found = book / "index.html", book / "404.html"
    if not index.is_file() or not not_found.is_file() or not index.stat().st_size or not not_found.stat().st_size:
        add(errors, "generated mdBook must include non-empty index.html and 404.html")
        return
    toc = book / "toc.html"
    if not toc.is_file() or not toc.stat().st_size:
        add(errors, "generated mdBook must include non-empty toc.html for sidebar validation")
        return
    parts = (
        "Part I — Getting Started",
        "Part II — Core Concepts",
        "Part III — Design & Architecture",
        "Part IV — Recipes & Applied Parsers",
        "Part V — Reference",
    )
    sidebar = sidebar_toc(toc)
    if tuple(sidebar.part_titles) != parts:
        add(errors, "mdBook sidebar is missing or reorders the five required parts")
    expected_hrefs = tuple(filename.replace(".md", ".html") for filename, _title in CHAPTERS)
    chapter_hrefs = tuple(
        href for href in sidebar.chapter_hrefs
        if re.fullmatch(r"(?:ch\d{2}|arch|ref|recipe)_[^/]+\.html", href)
    )
    if chapter_hrefs != expected_hrefs:
        add(errors, "mdBook sidebar chapter hrefs do not match the ordered 27 chapters")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", action="store_true", help="validate guide source and README")
    parser.add_argument("--book", type=Path, help="validate an mdBook output directory")
    args = parser.parse_args()
    if not args.source and args.book is None:
        parser.error("choose --source and/or --book")
    errors: list[str] = []
    if args.source:
        validate_summary(errors)
        validate_headings(errors)
        validate_links(errors)
        validate_examples(errors)
        validate_features(errors)
    if args.book is not None:
        validate_book(errors, args.book)
    if errors:
        for error in errors:
            print(f"validate_docs.py: {error}", file=sys.stderr)
        return 1
    print("validate_docs.py: OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
