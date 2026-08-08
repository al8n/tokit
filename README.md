<div align="center">
<h1>Tokora</h1>
</div>
<div align="center">

Deterministic parser combinators, with on-demand lexing, LALR-style dispatch, explicit backtracking, and configurable diagnostics.

[<img alt="github" src="https://img.shields.io/badge/github-al8n/tokora-8da0cb?style=for-the-badge&logo=Github" height="22">][Github-url]
<img alt="LoC" src="https://img.shields.io/endpoint?url=https%3A%2F%2Fgist.githubusercontent.com%2Fal8n%2F327b2a8aef9003246e45c6e47fe63937%2Fraw%2Ftokora" height="22">
[<img alt="Build" src="https://img.shields.io/github/actions/workflow/status/al8n/tokora/ci.yml?logo=Github-Actions&style=for-the-badge" height="22">][CI-url]
[<img alt="codecov" src="https://img.shields.io/codecov/c/gh/al8n/tokora?style=for-the-badge&token=6R3QFWRWHL&logo=codecov" height="22">][codecov-url]

[<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-tokora-66c2a5?style=for-the-badge&labelColor=555555&logo=data:image/svg+xml;base64,PHN2ZyByb2xlPSJpbWciIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgdmlld0JveD0iMCAwIDUxMiA1MTIiPjxwYXRoIGZpbGw9IiNmNWY1ZjUiIGQ9Ik00ODguNiAyNTAuMkwzOTIgMjE0VjEwNS41YzAtMTUtOS4zLTI4LjQtMjMuNC0zMy43bC0xMDAtMzcuNWMtOC4xLTMuMS0xNy4xLTMuMS0yNS4zIDBsLTEwMCAzNy41Yy0xNC4xIDUuMy0yMy40IDE4LjctMjMuNCAzMy43VjIxNGwtOTYuNiAzNi4yQzkuMyAyNTUuNSAwIDI2OC45IDAgMjgzLjlWMzk0YzAgMTMuNiA3LjcgMjYuMSAxOS45IDMyLjJsMTAwIDUwYzEwLjEgNS4xIDIyLjEgNS4xIDMyLjIgMGwxMDMuOS01MiAxMDMuOSA1MmMxMC4xIDUuMSAyMi4xIDUuMSAzMi4yIDBsMTAwLTUwYzEyLjItNi4xIDE5LjktMTguNiAxOS45LTMyLjJWMjgzLjljMC0xNS05LjMtMjguNC0yMy40LTMzLjd6TTM1OCAyMTQuOGwtODUgMzEuOXYtNjguMmw4NS0zN3Y3My4zek0xNTQgMTA0LjFsMTAyLTM4LjIgMTAyIDM4LjJ2LjZsLTEwMiA0MS40LTEwMi00MS40di0uNnptODQgMjkxLjFsLTg1IDQyLjV2LTc5LjFsODUtMzguOHY3NS40em0wLTExMmwtMTAyIDQxLjQtMTAyLTQxLjR2LS42bDEwMi0zOC4yIDEwMiAzOC4ydi42em0yNDAgMTEybC04NSA0Mi41di03OS4xbDg1LTM4Ljh2NzUuNHptMC0xMTJsLTEwMiA0MS40LTEwMi00MS40di0uNmwxMDItMzguMiAxMDIgMzguMnYuNnoiPjwvcGF0aD48L3N2Zz4K" height="20">][doc-url]
[<img alt="book" src="https://img.shields.io/badge/book-tokora-e5928d?style=for-the-badge&logo=mdbook" height="22">][tutorial-url]
[<img alt="crates.io" src="https://img.shields.io/crates/v/tokora?style=for-the-badge&logo=data:image/svg+xml;base64,PD94bWwgdmVyc2lvbj0iMS4wIiBlbmNvZGluZz0iaXNvLTg4NTktMSI/Pg0KPCEtLSBHZW5lcmF0b3I6IEFkb2JlIElsbHVzdHJhdG9yIDE5LjAuMCwgU1ZHIEV4cG9ydCBQbHVnLUluIC4gU1ZHIFZlcnNpb246IDYuMDAgQnVpbGQgMCkgIC0tPg0KPHN2ZyB2ZXJzaW9uPSIxLjEiIGlkPSJMYXllcl8xIiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHhtbG5zOnhsaW5rPSJodHRwOi8vd3d3LnczLm9yZy8xOTk5L3hsaW5rIiB4PSIwcHgiIHk9IjBweCINCgkgdmlld0JveD0iMCAwIDUxMiA1MTIiIHhtbDpzcGFjZT0icHJlc2VydmUiPg0KPGc+DQoJPGc+DQoJCTxwYXRoIGQ9Ik0yNTYsMEwzMS41MjgsMTEyLjIzNnYyODcuNTI4TDI1Niw1MTJsMjI0LjQ3Mi0xMTIuMjM2VjExMi4yMzZMMjU2LDB6IE0yMzQuMjc3LDQ1Mi41NjRMNzQuOTc0LDM3Mi45MTNWMTYwLjgxDQoJCQlsMTU5LjMwMyw3OS42NTFWNDUyLjU2NHogTTEwMS44MjYsMTI1LjY2MkwyNTYsNDguNTc2bDE1NC4xNzQsNzcuMDg3TDI1NiwyMDIuNzQ5TDEwMS44MjYsMTI1LjY2MnogTTQzNy4wMjYsMzcyLjkxMw0KCQkJbC0xNTkuMzAzLDc5LjY1MVYyNDAuNDYxbDE1OS4zMDMtNzkuNjUxVjM3Mi45MTN6IiBmaWxsPSIjRkZGIi8+DQoJPC9nPg0KPC9nPg0KPGc+DQo8L2c+DQo8Zz4NCjwvZz4NCjxnPg0KPC9nPg0KPGc+DQo8L2c+DQo8Zz4NCjwvZz4NCjxnPg0KPC9nPg0KPGc+DQo8L2c+DQo8Zz4NCjwvZz4NCjxnPg0KPC9nPg0KPGc+DQo8L2c+DQo8Zz4NCjwvZz4NCjxnPg0KPC9nPg0KPGc+DQo8L2c+DQo8Zz4NCjwvZz4NCjxnPg0KPC9nPg0KPC9zdmc+DQo=" height="22">][crates-url]
[<img alt="crates.io" src="https://img.shields.io/crates/d/tokora?color=critical&logo=data:image/svg+xml;base64,PD94bWwgdmVyc2lvbj0iMS4wIiBzdGFuZGFsb25lPSJubyI/PjwhRE9DVFlQRSBzdmcgUFVCTElDICItLy9XM0MvL0RURCBTVkcgMS4xLy9FTiIgImh0dHA6Ly93d3cudzMub3JnL0dyYXBoaWNzL1NWRy8xLjEvRFREL3N2ZzExLmR0ZCI+PHN2ZyB0PSIxNjQ1MTE3MzMyOTU5IiBjbGFzcz0iaWNvbiIgdmlld0JveD0iMCAwIDEwMjQgMTAyNCIgdmVyc2lvbj0iMS4xIiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHAtaWQ9IjM0MjEiIGRhdGEtc3BtLWFuY2hvci1pZD0iYTMxM3guNzc4MTA2OS4wLmkzIiB3aWR0aD0iNDgiIGhlaWdodD0iNDgiIHhtbG5zOnhsaW5rPSJodHRwOi8vd3d3LnczLm9yZy8xOTk5L3hsaW5rIj48ZGVmcz48c3R5bGUgdHlwZT0idGV4dC9jc3MiPjwvc3R5bGU+PC9kZWZzPjxwYXRoIGQ9Ik00NjkuMzEyIDU3MC4yNHYtMjU2aDg1LjM3NnYyNTZoMTI4TDUxMiA3NTYuMjg4IDM0MS4zMTIgNTcwLjI0aDEyOHpNMTAyNCA2NDAuMTI4QzEwMjQgNzgyLjkxMiA5MTkuODcyIDg5NiA3ODcuNjQ4IDg5NmgtNTEyQzEyMy45MDQgODk2IDAgNzYxLjYgMCA1OTcuNTA0IDAgNDUxLjk2OCA5NC42NTYgMzMxLjUyIDIyNi40MzIgMzAyLjk3NiAyODQuMTYgMTk1LjQ1NiAzOTEuODA4IDEyOCA1MTIgMTI4YzE1Mi4zMiAwIDI4Mi4xMTIgMTA4LjQxNiAzMjMuMzkyIDI2MS4xMkM5NDEuODg4IDQxMy40NCAxMDI0IDUxOS4wNCAxMDI0IDY0MC4xOTJ6IG0tMjU5LjItMjA1LjMxMmMtMjQuNDQ4LTEyOS4wMjQtMTI4Ljg5Ni0yMjIuNzItMjUyLjgtMjIyLjcyLTk3LjI4IDAtMTgzLjA0IDU3LjM0NC0yMjQuNjQgMTQ3LjQ1NmwtOS4yOCAyMC4yMjQtMjAuOTI4IDIuOTQ0Yy0xMDMuMzYgMTQuNC0xNzguMzY4IDEwNC4zMi0xNzguMzY4IDIxNC43MiAwIDExNy45NTIgODguODMyIDIxNC40IDE5Ni45MjggMjE0LjRoNTEyYzg4LjMyIDAgMTU3LjUwNC03NS4xMzYgMTU3LjUwNC0xNzEuNzEyIDAtODguMDY0LTY1LjkyLTE2NC45MjgtMTQ0Ljk2LTE3MS43NzZsLTI5LjUwNC0yLjU2LTUuODg4LTMwLjk3NnoiIGZpbGw9IiNmZmZmZmYiIHAtaWQ9IjM0MjIiIGRhdGEtc3BtLWFuY2hvci1pZD0iYTMxM3guNzc4MTA2OS4wLmkwIiBjbGFzcz0iIj48L3BhdGg+PC9zdmc+&style=for-the-badge" height="22">][crates-url]

[<img alt="Discord" src="https://img.shields.io/discord/835936528140206122?style=for-the-badge&logo=discord&logoColor=white&label=Discord&color=7289da" height="22">][discord]
<img alt="license" src="https://img.shields.io/badge/License-Apache%202.0/MIT-blue.svg?style=for-the-badge&fontColor=white&logoColor=f5c076&logo=data:image/svg+xml;base64,PCFET0NUWVBFIHN2ZyBQVUJMSUMgIi0vL1czQy8vRFREIFNWRyAxLjEvL0VOIiAiaHR0cDovL3d3dy53My5vcmcvR3JhcGhpY3MvU1ZHLzEuMS9EVEQvc3ZnMTEuZHRkIj4KDTwhLS0gVXBsb2FkZWQgdG86IFNWRyBSZXBvLCB3d3cuc3ZncmVwby5jb20sIFRyYW5zZm9ybWVkIGJ5OiBTVkcgUmVwbyBNaXhlciBUb29scyAtLT4KPHN2ZyBmaWxsPSIjZmZmZmZmIiBoZWlnaHQ9IjgwMHB4IiB3aWR0aD0iODAwcHgiIHZlcnNpb249IjEuMSIgaWQ9IkNhcGFfMSIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIiB4bWxuczp4bGluaz0iaHR0cDovL3d3dy53My5vcmcvMTk5OS94bGluayIgdmlld0JveD0iMCAwIDI3Ni43MTUgMjc2LjcxNSIgeG1sOnNwYWNlPSJwcmVzZXJ2ZSIgc3Ryb2tlPSIjZmZmZmZmIj4KDTxnIGlkPSJTVkdSZXBvX2JnQ2FycmllciIgc3Ryb2tlLXdpZHRoPSIwIi8+Cg08ZyBpZD0iU1ZHUmVwb190cmFjZXJDYXJyaWVyIiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiLz4KDTxnIGlkPSJTVkdSZXBvX2ljb25DYXJyaWVyIj4gPGc+IDxwYXRoIGQ9Ik0xMzguMzU3LDBDNjIuMDY2LDAsMCw2Mi4wNjYsMCwxMzguMzU3czYyLjA2NiwxMzguMzU3LDEzOC4zNTcsMTM4LjM1N3MxMzguMzU3LTYyLjA2NiwxMzguMzU3LTEzOC4zNTcgUzIxNC42NDgsMCwxMzguMzU3LDB6IE0xMzguMzU3LDI1OC43MTVDNzEuOTkyLDI1OC43MTUsMTgsMjA0LjcyMywxOCwxMzguMzU3UzcxLjk5MiwxOCwxMzguMzU3LDE4IHMxMjAuMzU3LDUzLjk5MiwxMjAuMzU3LDEyMC4zNTdTMjA0LjcyMywyNTguNzE1LDEzOC4zNTcsMjU4LjcxNXoiLz4gPHBhdGggZD0iTTE5NC43OTgsMTYwLjkwM2MtNC4xODgtMi42NzctOS43NTMtMS40NTQtMTIuNDMyLDIuNzMyYy04LjY5NCwxMy41OTMtMjMuNTAzLDIxLjcwOC0zOS42MTQsMjEuNzA4IGMtMjUuOTA4LDAtNDYuOTg1LTIxLjA3OC00Ni45ODUtNDYuOTg2czIxLjA3Ny00Ni45ODYsNDYuOTg1LTQ2Ljk4NmMxNS42MzMsMCwzMC4yLDcuNzQ3LDM4Ljk2OCwyMC43MjMgYzIuNzgyLDQuMTE3LDguMzc1LDUuMjAxLDEyLjQ5NiwyLjQxOGM0LjExOC0yLjc4Miw1LjIwMS04LjM3NywyLjQxOC0xMi40OTZjLTEyLjExOC0xNy45MzctMzIuMjYyLTI4LjY0NS01My44ODItMjguNjQ1IGMtMzUuODMzLDAtNjQuOTg1LDI5LjE1Mi02NC45ODUsNjQuOTg2czI5LjE1Miw2NC45ODYsNjQuOTg1LDY0Ljk4NmMyMi4yODEsMCw0Mi43NTktMTEuMjE4LDU0Ljc3OC0zMC4wMDkgQzIwMC4yMDgsMTY5LjE0NywxOTguOTg1LDE2My41ODIsMTk0Ljc5OCwxNjAuOTAzeiIvPiA8L2c+IDwvZz4KDTwvc3ZnPg==" height="22">

</div>

## Introduction

Tokora is a Rust parser-combinator library with on-demand lexing, explicit lookahead and
backtracking, configurable diagnostics, and optional Logos and Rowan integrations. Parsers work
over a `Lexer` and `Token` model, so the same grammar can use a fail-fast runtime emitter or a
collecting diagnostic emitter.

## Install

Most applications use the maintained Logos adapter:

```toml
[dependencies]
tokora = { version = "0.9", features = ["logos"] }
```

`logos` is the alias for the current `logos_0_16` integration. The default `std` feature remains
enabled unless you set `default-features = false`.

> **Which version this describes.** Every `[dependencies]` snippet in this README resolves against
> the latest release, **0.9.1**. A feature that `main` has grown since is named with the version it
> will arrive in, rather than put in a copyable block you cannot resolve.

## Capabilities

- On-demand token flow through `InputRef`, with explicit cache-backed lookahead and transactions.
- Plain parser functions plus composable sequencing, repetition, delimiters, and deterministic
  choice.
- Token-level and AST-level Pratt parsing.
- Configurable `Fatal`, `Verbose`, `Silent`, and `Ignored` diagnostics.
- Recovery, partial-input support, lexer conformance checks, tracing, and a public fuzz harness.
- Optional lossless CST building over the parser's own backtracking (feature `rowan`): a
  rewindable event-stream sink where a parser rollback rewinds the half-built tree, and `node`
  combinators bracket sub-parses into syntax nodes.
- Optional adapters for Logos, Rowan CSTs, source types, and container types.

## How Tokora parses

A Tokora grammar is ordinary Rust: parser functions and combinators read through `InputRef`, which
pulls tokens from a `Lexer` on demand and stages tokens in its cache when lookahead or backtracking
needs them. `peek_then_choice` makes a decision from a fixed lookahead window;
`dispatch_on_kind` and `fused_dispatch_on_kind` route the next token's `Token::Kind` to exactly
one selected branch.

That token-kind dispatch is local to a hand-written combinator grammar. Tokora does not accept an
LALR grammar, generate LALR parse tables, or act as an LALR parser generator.

When a grammar needs speculation, it is explicit. `attempt` and `try_attempt` commit successful
work and roll back a decline or error; `Transaction` exposes commit and rollback directly. A
rollback restores the input position, span, lexer state, token cache, and diagnostics emitted since
the checkpoint. Application-owned side effects need their own transaction boundary.

## Diagnostics and recovery

Parsers are generic over their parse context, including the emitter. `Parser::new()` uses the
fail-fast `Fatal` emitter; `Verbose` records diagnostics and can continue when the grammar
recovers. The same parser functions can therefore serve a runtime parser, compiler front end, or
editor integration without a second grammar implementation.

Structured lexer, token, separator, container, and Pratt errors convert into the application's
error type through `From` implementations.

Recovery is explicit: `recover` restores the failed parse's starting point before running a
recovery parser, while `inplace_recover` continues from the failure position. `sync_balanced` and
`skip_then_retry` provide nesting-aware synchronization; `Verbose` records each successful
non-empty skipped region once alongside other diagnostics. `Incomplete` errors are re-raised
instead of recovered so unfinished partial input is not discarded.

## Guide and examples

The [Tokora Guide](https://al8n.github.io/tokora/) has three parts: ten Calc fundamentals, one
anatomy chapter plus four maintained-example walkthroughs (five applied-parser chapters), and an
optional Rowan/lossless-CST chapter. The examples below are canonical complete programs; the guide
links back to them instead of copying whole files into prose.

| Program | Focus | Canonical source | Run |
| --- | --- | --- | --- |
| `calculator` | Token-level Pratt evaluator | [`calculator.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/calculator.rs) | `cargo run -p tokora --example calculator --features logos` |
| `s_expression` | Recursive descent and evaluation | [`s_expression.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/s_expression.rs) | `cargo run -p tokora --example s_expression --features logos` |
| `json` | Borrowed values, delimiters, and tentative choice | [`json.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/json.rs) | `cargo run -p tokora --example json --features logos` |
| `c_expression` | AST-level Pratt parsing with postfix forms | [`c_expression.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/c_expression.rs) | `cargo run -p tokora --example c_expression --features logos` |

The book source lives under
[`tokora/src/guide`](https://github.com/al8n/tokora/tree/main/tokora/src/guide), and the examples
also compile together with `cargo test -p tokora --no-default-features --features std,logos,combinators --examples`.

## Features

The combinator-family gates — `combinators` and the thirteen families it covers, `any` through
`validate` — are **new in 0.9.0**. On 0.8.0 and earlier there are no per-family gates, `default`
is `std` alone, and every combinator is compiled unconditionally.

| Feature | Effect |
| --- | --- |
| `default` | Enables `std` and `combinators`. |
| `std` | Enables standard-library support and default features of applicable dependencies. |
| `alloc` | Enables allocation-backed facilities in `no_std` builds. |
| `combinators` | Umbrella for every combinator family below. On by default. |
| `any` | `Any` — accept one token of any kind. |
| `fail` | `fail` / `fail_with`. |
| `filter` | `filter`, `filter_with`, `filter_map`, `filter_map_with`. |
| `fold` | The fold drivers (`fold_while`, `try_fold*`, `rfold*`); implies `many`. |
| `ident` | `Ident::parse` / `try_parse` and their `_except` twins. |
| `keyword` | `Keyword::parse` / `try_parse` and their `_exact` / `_sliced` twins. |
| `many` | The repetition family: `repeated*`, `separated*`, `delim*`, the delimiter handlers, the cardinality bounds, `list` and `separated1`. |
| `map` | `map` / `map_with`. |
| `peek` | `peek_then*`, `peek_then_choice`, `peek_kind`, `dispatch_on_kind` and its fused twin. |
| `pratt` | Pratt expressions: the typed `pratt` driver, the token-level `InputRef::pratt*`, `PrattToken`, and the `PrattEmitter` channel. |
| `punct` | The punctuator parsers (`Comma::parse`, …) and the `parens`/`braces`/`brackets`/`angles` delimited shapes built on them. |
| `then` | `then`, `then_ignore`, `ignore_then`, `then_value`, `and_then`, `and_then_with`. |
| `validate` | `validate` / `validate_with`. |
| `logos` | Alias for `logos_0_16`, the current Logos integration. |
| `logos_0_14` | Enables the optional `logos@0.14` adapter. |
| `logos_0_15` | Enables the optional `logos@0.15` adapter. |
| `logos_0_16` | Enables the optional `logos@0.16` adapter used by `logos`. |
| `trace` | Enables parser tracing; implies `std`. |
| `unstable-raw` | Exposes the unstable raw checkpoint API. |
| `conformance` | Enables the custom-lexer conformance test kit; implies `std`. |
| `fuzz` | Enables the deterministic public input/backtracking fuzz harness; implies `std`. |
| `rowan` | Enables the rewindable event-stream Rowan CST — emitter, recording sink, and `node` combinators; implies `std`. A lossless sink requires a trivia-surfacing lexer (`Lexer::SURFACES_TRIVIA`). Add `rowan = "0.17"` directly when implementing `rowan::Language`. |
| `bytes` | Alias for `bytes_1`. |
| `bytes_1` | Enables `bytes@1` source support. |
| `bstr` | Alias for `bstr_1`. |
| `bstr_1` | Enables `bstr@1` source support. |
| `hipstr` | Alias for `hipstr_0_8`. |
| `hipstr_0_8` | Enables `hipstr@0.8` source support. |
| `smol_bytes` | Alias for `smol_bytes_0_1`. |
| `smol_bytes_0_1` | Enables `smol-bytes@0.1` source support (smol-bytes ≥ 0.1.2). |
| `smallvec` | Alias for `smallvec_1`. |
| `smallvec_1` | Enables `smallvec@1` containers and implies `alloc`. |
| `heapless` | Alias for `heapless_0_9`. |
| `heapless_0_9` | Enables `heapless@0.9` containers. |
| `tinyvec` | Alias for `tinyvec_1`. |
| `tinyvec_1` | Enables `tinyvec@1` containers. |

Every combinator family is independently gateable so an embedded or no-alloc build compiles only
the combinators it calls. What the families sit on stays unconditional: `Parser`/`Parse`/`parse*`,
the `ParseInput` / `TryParseInput` / `ParseChoice` traits, and the substrate combinators
(`expect`, `delimited`, `recover`, `select`, `opt`, `padded`, `node`, `labelled`, …). `combinators`
is a default feature, so a plain dependency line sees the whole surface; a `default-features = false`
build names the families it uses.

Feature aliases select their versioned counterpart; versioned features make the corresponding
optional dependency available. One Logos version is normally sufficient, though multiple versioned
integrations may coexist. When several are enabled, the unversioned
`tokora::lexer::LogosLexer` selects 0.16, then 0.15, then 0.14. `tokora::logos` is available only
with `logos_0_16` and re-exports that version. `rowan` does not enable `logos`, and `smallvec_1`
is the versioned feature that adds `alloc`.

## Platform support

Tokora's MSRV is Rust 1.95.
Tokora's core supports both allocator-free `no_std`
(`no_std` without `alloc`) and allocation-enabled `no_std` (`no_std` with `alloc`).
Disable default features for allocator-free core use. Enable `alloc` when a parser, cache,
or selected optional facility requires allocation; other optional facilities may require `std`.

Allocator-free `no_std`:

```toml
[dependencies]
tokora = { version = "0.9", default-features = false }
```

`no_std` with `alloc`:

```toml
[dependencies]
tokora = { version = "0.9", default-features = false, features = ["alloc"] }
```

Neither line enables a combinator family: `combinators` is a default feature, and both turn the
defaults off. Add `features = ["combinators"]` for the umbrella, or list the families the grammar
actually calls, as in `features = ["alloc", "many", "map"]`.

## Design philosophy and inspirations

### Core Priorities

1. **Performance** - Pull tokens from the lexer on demand and offer fused dispatch where avoiding a peek/cache round trip matters.
2. **Predictability** - Prefer deterministic lookahead and token-kind dispatch; make speculation explicit and transactional.
3. **Composability** - Combine small parser functions and combinators; compose focused emitter traits into custom diagnostic strategies.
4. **Versatility** - Reuse parser functions with fail-fast, collecting, silent, or custom emitters.
5. **Flexibility** - Work through generic `Lexer` and `Token` traits, with optional Logos input and Rowan CST integrations.
6. **Correctness** - Track spans and structured errors, rewind emitted diagnostics with parser rollbacks, and provide conformance and fuzz test kits.

### Inspirations

Tokora takes inspiration from:

- [**winnow**](https://github.com/winnow-rs/winnow) - For ergonomic parser API design
- [**chumsky**](https://github.com/zesterer/chumsky) - For composable parser combinator patterns
- [**logos**](https://github.com/maciejhirsz/logos) - For high-performance lexing
- [**rowan**](https://github.com/rust-analyzer/rowan) - For lossless syntax tree representation

## Development

Useful repository checks:

```sh
cargo fmt --all --check
cargo test -p tokora --all-features
cargo test -p tokora --no-default-features --features std,logos,combinators --examples
RUSTDOCFLAGS="-D warnings" cargo test -p tokora --all-features --doc
(cd tokora && mdbook build)
```

The guide is validated both as rustdoc and as an mdBook so API links, local links, chapter order,
and Pages output stay aligned.

## License

`tokora` is under the terms of both the MIT license and the
Apache License (Version 2.0).

See the [Apache License, Version 2.0](LICENSE-APACHE) and the [MIT license](LICENSE-MIT) text
for details.

Copyright (c) 2026 Al Liu.

[Github-url]: https://github.com/al8n/tokora/
[CI-url]: https://github.com/al8n/tokora/actions/workflows/ci.yml
[doc-url]: https://docs.rs/tokora
[crates-url]: https://crates.io/crates/tokora
[codecov-url]: https://app.codecov.io/gh/al8n/tokora/
[discord]: https://discord.gg/FTuwh4d4N7
[tutorial-url]: https://al8n.github.io/tokora
