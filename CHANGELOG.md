# Changelog

All notable changes to this crate are documented here. The project follows semantic
versioning; before 1.0, a minor bump (0.x → 0.(x+1)) signals a breaking change.

<!--
Cross-references inside this file NEVER use a bare GitHub heading slug. Six headings here
are spelled `### Changed (breaking)` and seven `### Added`; GitHub numbers duplicate slugs in
document order, so `#changed-breaking` means "whichever one comes first today" and prepending
a section silently re-points every link that used it. It has happened: 53 references into
0.8.0 came to point at a one-item stub the moment `## Unreleased` was opened above them.

So a link target is an explicit anchor, declared on its own line one blank line above the
heading it names, and named `<section-key>-<heading-slug>`:

    <a id="0.8.0-changed-breaking"></a>

    ### Changed (breaking)

and referenced as `[16](#0.8.0-changed-breaking)`. The blank line is required. CommonMark
reads `<a id="…"></a>` as a paragraph of inline HTML, and a renderer that instead reads a
lone tag on its own line as an HTML *block* would swallow the line below it — the blank line
is correct under both readings, and it gives the gate one deterministic place to look.

When `## Unreleased` becomes `## 0.9.0 (date)`, its anchors and the links to them change key
with it. `ci/changelog_structure.sh` enforces every clause above and will red until they do.
-->

## 0.8.0 (2026-08-04)

The whole of a 52-defect audit campaign lands in one release. Entries are grouped by **kind**, not by the round that produced them: a reader upgrading wants every breaking change in one place. Round provenance rides as an inline tag — *(R7, #117)* — and the pull-request bodies carry the full trail.

### Upgrading from 0.7.3 — the migration table

Every break a 0.7.3 consumer can hit, in one place. **Rows are ordered by how the break
announces itself, not by how many consumers hit it** — the ones your build will *not* point at
come first, because those are the ones you have to go looking for. Each row links to the
numbered entry below that carries the full reasoning.

#### These do not fail to compile. Read them first.

| Old behaviour | New behaviour | Why, in one line | Item |
|---|---|---|---|
| A `SimpleSpan` const mutator silently published an inverted or wrapped span — `with_end_const(2)` on `(5, 15)` gave `(5, 2)`; `bump_start_const` at `(MAX-1, MAX)` gave `(0, usize::MAX)` in release | panics: `end must be greater than or equal to start` / `span bump overflows usize` | The span was **already corrupt** and the program was carrying it. The panic is the bug becoming visible, not a new refusal. | [36](#0.8.0-changed-breaking) |
| A properly closed `separated_by_*().delimited()` list ignored `at_least` / `at_most` / `bounded` and the separator policy | the policy runs: a recovering emitter records the diagnostic, a fail-fast one returns `Err` where it returned `Ok` | The end-state pass was dead code on that exit only. | [1](#0.8.0-changed-breaking) |
| `TooMany` carried the running count, or the limit itself (`found 2 … exceeds … 2`) | `limit() + 1`, emitted once per construct | Payloads that contradicted themselves. | [2](#0.8.0-changed-breaking) |
| `FullContainer` re-emitted per dropped element, counting past its own capacity | once per construct, on the whole-construct span | | [3](#0.8.0-changed-breaking) |
| Bounded containers turned a satisfied `at_least` into `TooFew`, and swallowed a violated `at_most` | bounds judge the elements **parsed**, not the ones stored | Only bounded-capacity containers were affected. | [4](#0.8.0-changed-breaking) |
| `SeparatorHandler::on_separator` received only leading and duplicate separators | receives every separator in source order | It received the exact complement of its documented contract. Opt out with `OBSERVES_SEPARATORS`. | [5](#0.8.0-changed-breaking) |
| Delimited separated drivers returned a span measured from the first element on some exits | the whole construct's span, opener through closer, on every success exit | | [6](#0.8.0-changed-breaking) |
| A dialect mapper returning an out-of-language kind reached `finish` as `Err(InvalidDialectKind)` | panics at the emit site, in every build | It is a dialect bug, not an input condition; no parse input can provoke it. | [14](#0.8.0-changed-breaking) |
| A panic in caller code during `skip_until` lost the in-flight token and the skipped prefix | the stream is left consistent on the panic path too | Only observable to a host that catches a panic across a parse and keeps using the input. | [9](#0.8.0-changed-breaking) |
| `Emitter::commit_token`'s observer ran before the committed position was published | runs **after** the position is published, before the replaced pair is dropped | An observer that panics no longer leaves the input naming a token the stream does not hold. | [10](#0.8.0-changed-breaking) |
| A **wrapper** emitter over a recording `Sink` forwarded lexer errors by forwarding `emit_lexer_error` | must also forward the new `Emitter::commit_lexer_error`, exactly as it forwards `commit_token` | The input layer's own refusals now arrive through that method, and their spans are what license an untokenized byte. A wrapper that inherits the default turns them into caller reports, and the wrapped sink's `finish` then refuses a legitimately unlexable region as `UncoveredGap`. Diagnostics-only emitters are unaffected — the default forwards to `emit_lexer_error`. | [68](#0.8.0-changed-breaking) |
| A parser or callback that reported a lexer error over an **unconsumed** region — `inp.emit_lexer_error(span)`, `view.emit_lexer_error(span)` — thereby licensed those bytes to be gap-tiled, and `finish` returned a tree | the report is delivered unchanged; it licenses nothing, so `finish` refuses the uncovered region as `UncoveredGap` | A recorded lexer-error span is a *licence*, and a caller-chosen span with nothing consumed for it is the deleted `cst_token` shape on the diagnostic channel. Materialize an unconsumed region with `Cst::finish_partial`, which tiles it. | [68](#0.8.0-changed-breaking) |
| An `Incomplete` scan exit committed the frontier it reached | exits as it entered | A refill driver resumed at a half-consumed position. | [12](#0.8.0-changed-breaking) |
| A terminal scanner stop (resource limit, poison boundary) reached callers as an ordinary end-of-input **decline** | surfaces as a terminal error **when your emitter accepts the diagnostic**, and recovery re-raises it instead of retrying. A *rejecting* emitter's `Err` is built from your lexer's error value and carries no mark, so recovery can still spend that trip — `MaybeTerminal`'s doc has that path and the arm it needs | Recovery was retrying against a scanner that had already given up. | [16](#0.8.0-changed-breaking) |
| Collection drivers could stall on zero progress, or mask a terminal stop as a successful end | both exits are errors | | [17](#0.8.0-changed-breaking) |
| `…, expected expected '}'` in `MissingToken` and `UnexpectedToken` renders | `…, expected '}'` | `Expected`'s own `Display` already supplies the word. **Re-bless frozen renders.** | [37](#0.8.0-changed-breaking) |
| `UnexpectedEnd`'s derived `Debug` / `Eq` / `Hash` | include a `terminal: bool` field | Shipped four rounds ago and disclosed nowhere until now. **Re-bless frozen renders.** | [16](#0.8.0-changed-breaking) |
| `Unclosed`'s derived `Debug`; `SeparatedError` / `MissingToken` derived `Debug`, `Eq`, `Hash` | include `kind` / the name channel | See *Debug and rendered output* below for the complete list. | [20](#0.8.0-changed-breaking), [28](#0.8.0-changed-breaking) |
| A slice-choice id out of range panicked with `index out of bounds` | `choice id {id} out of bounds for {len} branches` | Message text only; reachability is unchanged. | [35](#0.8.0-changed-breaking) |
| A wrong opening delimiter produced **two** diagnostics naming the same token — once as the wrong opener, once again as a close miss | exactly one | Only under a recording emitter, and only while the first report is still live in the log. Assertions that counted diagnostics on the recovering path change. | [40](#0.8.0-changed-breaking) |
| A second same-power `PrattInfix::Neither` operator in one chain folded left in silence — `7 = 1 ; 2 ; 3` returned `Ok(((7=(1;2));3))` with the **whole input consumed**, so no end-of-input check had anything to catch | the parse fails with `NonAssociativeChain`, the operator left on the input unconsumed | Handing the operator up re-associates the chain across an enclosing frame that cannot know the constraint. Not terminal: recovery may still spend it. | [41](#0.8.0-changed-breaking) |
| Recursive descent was unbounded — a deep enough expression exhausted the native stack and **aborted the process** | a shared per-input depth budget, **64** by default, failing the parse with the always-terminal `RecursionLimitReached` | An abort carries no diagnostic and cannot be caught; a refusal names the knob that raises it. `RecursionLimiter::unlimited()` restores 0.7.3's behaviour. | [42](#0.8.0-changed-breaking) |
| `RecursionLimiter::new` / `Default`, and `Limiter::new` / `Limiter::with_token_tracker`, defaulted to depth **500** | **500** — unchanged, so there is nothing to do here. [43](#0.8.0-changed-breaking) dropped these four to 64 during the campaign and [50](#0.8.0-changed-breaking) returned them before release; only a build tracked between the two ever saw 64 | These constructors reach code with no pratt parser in it: the same type doubles as a **lexer-side** nesting tracker in a `State` / `Extras` position, where a level costs no native stack and 64 was a number chosen for a reason that does not apply. The depth that *does* change is the input layer's, in the row above. | [43](#0.8.0-changed-breaking), [50](#0.8.0-changed-breaking) |
| Your own `emit_error` / `peek_kind` / `labelled` / `opt` / … resolved to *your* method | may resolve to tokora's, with **no diagnostic at the call site** | 130 new inherent items and 14 new defaulted trait methods enter the method space, and **67 of the inherent ones sit on types you already had** — that 67 is the exposure table, and it is the list to read. 38 of the 67 land on `InputRef` and `ParseState` under the emitter's own method names, which is where a 0.7.3 shortcut helper over `inp.emitter()` sat. Whether yours still wins is decided by rustc's **pick order**, not by inherent-ness. | [source-breaking additions that can change behaviour with *no diagnostic at the call site*](#0.8.0-source-breaking-additions-that-can-change-behaviour-with-no-diagnostic-at-the-call-site) |
| — | a new `warning: unused import` naming one of *your* combinator traits | That warning is the **only** breadcrumb the silent case gives you: the steal stranded the import. | [source-breaking additions that can change behaviour with *no diagnostic at the call site*](#0.8.0-source-breaking-additions-that-can-change-behaviour-with-no-diagnostic-at-the-call-site) |

#### These fail to compile. Your build will point at every one.

| Old spelling | New spelling | Why, in one line | Item |
|---|---|---|---|
| `ParseCtx` | `ComposableParseContext` | Renamed and completed with `FromTokenErrors`, so one bound elaborates to the whole leaf surface. | [27](#0.8.0-changed-breaking) |
| `X::parse_of`, `X::try_parse_of`, `list_of`, … (167 items) | `X::parse`, `X::try_parse`, `list`, … | `Lang` is inferred from the input. A call site that spelled its generics gains one argument in last position; `_` is almost always enough. | [29](#0.8.0-changed-breaking) |
| `Parser<…, Error, …>` with eight constructors | `Parser` without the `Error` phantom; four constructors | | [32](#0.8.0-changed-breaking) |
| `E: From<Unclosed<Paren, …>> + From<Unclosed<Brace, …>> + …` | `E: FromUnclosed<'inp, L, Lang>` | One bound covers every pair. A residual per-delimiter bound now fails as a bare `E0277` — `From` is foreign and cannot be annotated. | [28](#0.8.0-changed-breaking) |
| `match err.name_ref() { "[]" => … }` | `match err.kind() { DelimiterKind::Bracket { .. } => … }` | A display name was never a dispatch key: `"[]"` is correct for *any* bracket-shaped pair. Custom pairs declare `DelimiterKind::Custom`. | [28](#0.8.0-changed-breaking) |
| `impl Delimiter for MyPair` | `const KIND: DelimiterKind = DelimiterKind::Custom("…")` required | No default: a defaulted identity is one the author never chose. | [28](#0.8.0-changed-breaking) |
| `Unclosed::new(span, name)` | `Unclosed::new(span, kind, name)` | Or reach for `Unclosed::paren` and siblings. | [28](#0.8.0-changed-breaking) |
| `Paren<(), (), LangA>` satisfying `Delimiter<'_, L, LangB>` | marker brand **is** the context language | A pair or separator copy-pasted out of a sibling dialect used to type-check silently. | [30](#0.8.0-changed-breaking), [33](#0.8.0-changed-breaking) |
| `UnclosedParen<S, Lang>` = `Unclosed<Paren<(),(),()>, S, Lang>` | `Unclosed<Paren<(),(),Lang>, S, Lang>` | Unchanged at `Lang = ()`. | [33](#0.8.0-changed-breaking) |
| `T::Kind: 'static` restated at five projections | hoisted to the `Token` trait | | [31](#0.8.0-changed-breaking) |
| `match rhs { … }` over `PrattRHS` | add an `End` arm | Deliberately not `#[non_exhaustive]`: a wildcard arm is the silent end-swallowing this removes. | [21](#0.8.0-changed-breaking) |
| `PrattPower::next` / `prev` | removed | They were the arithmetic that made the floor bugs expressible. Breaks callers **and** implementors. | [24](#0.8.0-changed-breaking) |
| `impl Cache { fn rewind(…) }` | delete it | Rollback had two possible owners and they disagreed. No caller loses a capability. | [7](#0.8.0-changed-breaking) |
| A `Cache` whose `push_front` can panic | must not panic on the restore path | Vacuously conforming before; not now. | [8](#0.8.0-changed-breaking) |
| A `Lexer` whose `Span`/`Offset` ops can panic on the settle path | must not panic | The one window in the consume path that cannot be closed by reordering. `Clone`, not `Copy`, is still the bound. | [11](#0.8.0-changed-breaking) |
| `Sink::new(emitter, map, ERR, GAP)` then `sink.finish(ROOT, source)` | `parse_lossless(source, state, emitter, CstProfile::new(map, ERR, GAP), cache, parser)` then `cst.finish(ROOT)` | Two independent names for one buffer is what let them disagree; [13](#0.8.0-changed-breaking) named it once at `Sink::new`, and [68](#0.8.0-changed-breaking) superseded that spelling before release by making `Sink::new` crate-private and minting the sink from the source itself — only a build tracked between the two ever called `Sink::new` directly. | [13](#0.8.0-changed-breaking), [68](#0.8.0-changed-breaking) |
| A callback parameter typed `&mut Ctx::Emitter` | `EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>` | Every `*_while` condition (`Decision::decide`), `ParseInput::peek_then`'s handler, `ParseChoice::peek_then_choice` / `peek_then_try_choice`'s handler, and the three token-level pratt folds. The view carries the emitter's own method names, so bodies do not change: **43 one-line signature changes, zero body changes, zero call-site changes** on the reference consumer. The same four getters — `peek_with_emitter`, `peek_with_emitter_terminal`, `sync_through_then_peek_with_emitter`, `sync_to_then_peek_with_emitter` — hand back a view in the tuple's second slot, so only a site that *named* that type breaks. | [68](#0.8.0-changed-breaking) |
| `inp.emitter().emit_error(e)`, `state.emitter().cst_start(k)` | `inp.emit_error(e)`, `state.cst_start(k)` | `InputRef::emitter` is crate-private (`E0624`) and `ParseState::emitter` is deleted (`E0599`). Both handles gained the emitter's operations under the emitter's own names — 25 methods on `InputRef`, 13 on `ParseState` — so the fix is deleting `.emitter()` from the call. `InputRef::emitter_ref()` is the read-only replacement where you needed the value. **Those 38 names can also steal a same-named method of yours; see the exposure table.** | [68](#0.8.0-changed-breaking) |
| `m.complete(inp.emitter(), KIND)` | `inp.cst_start_at(m.mark(), KIND); inp.cst_finish(KIND);` | `Marker::complete` takes `&mut E: CstEmitter` and a parse handle no longer hands one out, so `CompletedMarker` and `precede` are out of a grammar's reach too. Nothing is removed: the verb still works for code owning an emitter. The raw pair does not consume the marker, so the single-use typestate stops enforcing over that route. | [68](#0.8.0-changed-breaking) |
| `emitter.cst_token(span, kind)` | delete it | Deleted, not deprecated. A token event is born from a settle; `Emitter::commit_token` is now the only producer. Nothing in the parse machinery ever called it. | [68](#0.8.0-changed-breaking) |
| `emitter.cst_finish()` | `emitter.cst_finish(KIND)` | The kind the matching `cst_start` used. It is a **defaulted** method, so only callers and overriding implementors are affected. *Found by the mechanical API diff; disclosed nowhere before.* | [15](#0.8.0-changed-breaking) |
| `Sink<'_, L, Sink<…>>` | a compile error — the inner emitter must be `ValueKeyedEmitter` | Two mark spaces claiming the same keys. | [18](#0.8.0-changed-breaking) |
| `inp.begin_point(); … inp.commit_point();` | `let p = inp.begin_point(); … inp.commit_point(p);` | Nothing tied a settle to the point it settled. Points settle newest-first; four misuse conditions panic by name. | [19](#0.8.0-changed-breaking) |
| `let (o, e, m) = missing.into_components();` | `let (o, e, m, name) = …` | And `SeparatedError`'s pair becomes a triple. `..` is not available on a tuple pattern, so the compiler points at every site. | [20](#0.8.0-changed-breaking) |
| An error type without `From<UnexpectedEot<…>>` on the peek / `Expect` / fold / `Repeated` surfaces | add it, or name `ComposableParseContext` | A terminal stop has to be expressible as an error. | [16](#0.8.0-changed-breaking), [17](#0.8.0-changed-breaking) |
| An error type without `MaybeTerminal` | `impl MaybeTerminal for MyError {}` is enough | Unless the type can itself carry a terminal stop, and **three** that this crate builds and marks can reach it: an `UnexpectedEnd` whose flag the scanner may raise; a `RecursionLimitReached` — new in this release, required of every pratt-driving error type — terminal for **every** value; and a `SessionRefusal`, terminal for every value too but *not* a `MaybeTerminal` implementor, so its arm is written `true` by hand. Answer for each one you store. An arm left at `false` is spent as a recoverable failure — except the refusal, which `PartialSession::parse` asserts on unconditionally, so that one panics a release build. Those three are what the crate knows it produces, **not** a proof that nothing else is terminal: a scanner trip a *rejecting* emitter refuses propagates as that emitter's `Err`, built from your lexer's error value and unmarked, so the arm holding your lexer error may need answering too — `MaybeTerminal`'s doc has that path and the rule for it. | [16](#0.8.0-changed-breaking), [46](#0.8.0-changed-breaking) |
| `<[P; N] as ParseChoice>::Id::new(i)` (0-based) | `::new(i + 1)` (1-based) | `RangedUsize`'s bounds are inclusive, so the old id space admitted `N` and every `[P; 0]` id panicked. Tuple choices are unaffected. | [35](#0.8.0-changed-breaking) |
| `match op { … }` over `fuzz::Op` without a wildcard | add an `IsExhausted` arm | `fuzz` feature only. *Found by the mechanical API diff; disclosed nowhere before.* | [39](#0.8.0-changed-breaking) |
| `dyn SeparatorHandler` | no longer object-safe | `OBSERVES_SEPARATORS` is an associated const. Nothing in this crate used it. | [5](#0.8.0-changed-breaking) |
| `let (e, c) = ctx.into_components();` | `let (e, c, recursion) = …` | `InputContext` carries the recursion budget now, and a decomposition that dropped it would hand the input an unconfigured one. `..` is not available on a tuple pattern, so the compiler points at every site. | [45](#0.8.0-changed-breaking) |
| An error type driving either pratt engine without `From<RecursionLimitReached<…>>` and `From<NonAssociativeChain<…>>` | add both | The engines **return** these two rather than emitting them, so the entry-point bounds ask for them. Two `From` impls; every example in this repo shows the shape. | [46](#0.8.0-changed-breaking) |
| `parser.labelled("x")` resolving to your own trait's method, that trait in scope | `E0034`, when yours sits at the **same pick** | Fourteen new defaulted names on traits you already had — `ParseInput`, `TryParseInput`, `Emitter`, `SeparatorHandler`. The fix is UFCS; rustc prints the two suggestions. | [source-breaking additions that fail *loudly*](#0.8.0-source-breaking-additions-that-fail-loudly) |
| `use tokora::*;` — or a **module** glob such as `use tokora::error::*;` / `use tokora::cst::*;` — beside another glob exporting the same name | `E0659`, or `ambiguous_glob_imports` for a *macro* name on rustc ≥ 1.95 | 53 new glob-reachable names — `select!` against `tokio::select!` is the real one. Twenty are enumerated by path in that section; **nine of those twenty reach you only through a module glob, never the root**: `dispatch_take` and `try_dispatch_take` via `tokora::parser`; `kinds`, `Cst`, `parse_lossless` and `parse_lossless_partial` via `tokora::cst`; `RecursionLimitReached` and `NonAssociativeChain` via `tokora::error`; `Descent` via `tokora::input`. `EmitterView` reaches you from the root **and** from `tokora::emitter`. Measured on three toolchains; the remedy differs by name kind. | [source-breaking additions that fail *loudly*](#0.8.0-source-breaking-additions-that-fail-loudly) |

#### Additive — nothing to do, listed so you know it exists

`PolicyComposableEmitter` / `PolicyParseContext` (the count- and separator-policy bundle tier)
· `Emitter::bound_source` and `Source::REFERENT_IS_BYTES` (both defaulted) · `SourceIdentity`,
with `addr()` / `extent()` / `covers()` · `conformance::cache`
and `conformance::emitter` · `Dialect` · `CstText` · `PartialSession` and the streaming
lifecycle · `SeparatedError: Display` · `InputRef::is_exhausted()` · `Cache::RETAINS_FRONT` ·
`BStr` in the `Equivalent` family · `CstProfile` / `KindValidator` un-gated from `rowan` ·
`Silent`'s pratt impl loses four bounds it never used.

That list is additive **in the type system**. The names added below are not: adding a name
is a source break, and the two *Source-breaking additions* sections below say which
diagnostic you will get.

#### If you froze `Debug` or `Display` output

Every movement is listed in one place under **Debug and rendered output** below. A
private-field `Debug` delta broke a real consumer's frozen renders and cost a bisect during
this campaign, which is why that list exists at all.

#### Cross-references from earlier design documents

This release's `## 0.8.0` section was consolidated from twenty `###` headings carrying
five independently-numbered lists, so numeric cross-references written before the cut
("breaking change 3") do not resolve. The pull-request body carries the full old→new map.

### The new names, and why adding a name is still a break

Measured end to end, 0.7.3 against this release, this release adds **53 glob-reachable
root/module names**, **two new public modules** (`tokora::cst::kinds` and `tokora::dialect`,
each its own glob namespace), **23 trait-declared items** — 15 receiver methods, 2 associated
functions and 6 associated consts/types — and **130 inherent items — 119 receiver methods and
11 associated functions**, which resolve by different rules and are listed separately below for
that reason. An "inherent item" is counted once per `(name, owner type)` pair, because that pair
is what a collision is about: the same name on two owners is two questions a consumer answers
separately. The items themselves are under [Added](#0.8.0-added) with the rest of the release.

**Only part of that can collide with something you wrote, and the split is the useful number.**
**67 of the 130 inherent items — 64 methods and 3 associated functions — land on a type that
already existed in 0.7.3**; the other 63 land on types this release itself introduces, where no
extension method of yours can exist because you could not name the type. The exposure table
below is exactly those 67. On the trait side the same split is 13 of the 15 new methods, all of
them **defaulted**, on traits you already had (`ParseInput`, `TryParseInput`, `Emitter`,
`SeparatorHandler`); the other two are on new traits, which have to be imported before they can
compete.

**One item accounts for 72 of the 130, and it is a different shape from the rest of this
section.** [68](#0.8.0-changed-breaking) replaces `&mut Ctx::Emitter` with forwarded operations,
so the emitter's own method names land, in bulk, on the handles that used to hand the emitter
out: **25 on `InputRef`, 13 on `ParseState`, 27 on the new `EmitterView`, 7 on the new `Cst`.**
Only the first two are in the exposure table, for the reason above. The 38 that *can* collide
are all thin forwards of `Emitter` / `CstEmitter` methods under the emitter's own names, which
makes them the section's own worst case rather than an unusual one: `inp.emit_error(..)` is
exactly the shortcut a 0.7.3 consumer would have written over `inp.emitter().emit_error(..)`, on
exactly that receiver, with exactly a compatible signature. Read *the dangerous case is the one
that reads as safest*, below, as describing the expected outcome here and not a corner.

**Where those figures come from, and the correction they carry.** All of them are
`ci/name_collision/surface_diff.py` over rustdoc-JSON dumps of `468f7aa` (the 0.7.3 release
commit) and this branch's tip, at the `std,logos,trace,rowan` feature point — one run, one base,
one tool. **Earlier drafts of this section said 16 glob-reachable names, one new module, 6 trait
methods and 16 inherent items.** Each of those was true of the diff it was taken from — a
mid-campaign base, not 0.7.3 — and was then carried forward unchanged while the release kept
growing, which is the "second inventory that falls behind the first" this section warns about,
committed by this section. Re-running against 0.7.3 for [68](#0.8.0-changed-breaking) is what
surfaced it. **The enumerated lists further down are subsets and are labelled as such**; the
exposure table is not — it is the full 67, and the thirteen rows the re-audit added to it
(`InputRef::{is_exhausted, next_or_stop, try_expect_map_or_stop, peek_with_emitter_terminal}`,
`SeparatedError::{name, with_name, display_fmt}`, `UnexpectedEnd::{is_terminal, into_terminal}`,
`MissingToken::{name, with_name}`, `Unclosed::kind`, `Transaction::rollback_abandoning_points`)
are all new `(name, owner)` pairs on 0.7.3 types. Most were already documented elsewhere here as
*features* and simply never listed as names that can take a call site; one,
**`Transaction::rollback_abandoning_points` — a public method that rolls a transaction back while
abandoning the session points opened inside it — appears nowhere else in this changelog at all.**
*Found by the mechanical API diff; disclosed nowhere before.*

**A removal is part of this accounting too**, because it hands a name *back*: a consumer's
own `fn emitter(..)` on `InputRef` lost to tokora's inherent one in 0.7.3 and wins again here.
Measured against 0.7.3 on the same diff, [68](#0.8.0-changed-breaking) takes five inherent
items and one trait method off the surface — `Sink::new` (an associated function) and
`InputRef::emitter` become crate-private, `ParseState::emitter` is deleted, `Sink::finish` and
`Sink::finish_partial` move to `Cst` (a removal from `Sink` and an addition on `Cst`, counted
as both), and the trait method `CstEmitter::cst_token` is deleted. A sixth,
`Sink::with_trivia_policy`, leaves `Sink` for `Cst` on the same diff but was never in 0.7.3, so
it is an intra-release movement rather than a break a 0.7.3 consumer can hit. The release's
other removals are listed with the items that make them.

### The rule for new names, stated as weakly as it can honestly be stated

> **Any new public name can silently re-resolve a same-named item in your code. Whether
> you are told depends on spelling details that belong to the compiler, not to this
> crate.**

Four successively more precise versions of this rule were written during development and
each was falsified — never by a wrong mechanism, always by a **spelling nobody wrote down**.
The last one shipped a trait on the strength of "this name cannot collide silently, because
its type parameter never infers"; a turbofish supplies it, and discarding the return made
the collision silent with no error and no warning. That trait is not in this release.

So the rule above is the only form of it worth relying on, and the practical advice is
**detection, not prediction**:

> **If you have a method, associated function, free function, type or macro named any of
> the names in the two sections below, check that call site after upgrading.** The names
> are the complete generated list.

Everything after this point is *what was measured*, on the toolchains named. It is useful
for debugging an actual collision. It is not a guarantee, and a spelling not listed here is
not thereby safe.

<a id="0.8.0-source-breaking-additions-that-can-change-behaviour-with-no-diagnostic-at-the-call-site"></a>

### Source-breaking additions that can change behaviour with *no diagnostic at the call site*

#### Are you exposed? This question terminates.

**You are exposed if you have written a method or associated function with one of the names
below, on one of these receiver types:**

| you wrote it on | which names |
|---|---|
| `InputRef` | `try_expect_take`, `try_expect_take_or_stop`, `try_expect_map_or_stop`, `peek_head_map`, `head_satisfies`, `peek_kind`, `attempt_parse`, `spanning`, `descending`, `descend`, `recursion`, `is_exhausted`, `next_or_stop`, `peek_with_emitter_terminal` — and, from [68](#0.8.0-changed-breaking)'s forwarding surface, `emit_error`, `emit_warning`, `emit_lexer_error`, `emit_unexpected_token`, `emit_skipped_region`, `emit_too_few`, `emit_too_many`, `emit_full_container`, `emit_missing_element`, `emit_missing_separator`, `emit_missing_leading_separator`, `emit_missing_trailing_separator`, `emit_unexpected_leading_separator`, `emit_unexpected_trailing_separator`, `emit_unclosed`, `emit_unexpected_end_of_lhs`, `emit_unexpected_end_of_rhs`, `enter_label`, `exit_label`, `cst_start`, `cst_finish`, `cst_mark`, `cst_start_at`, `emitter_bound_source`, `emitter_ref` |
| `ParseState` — the callback view, which had no row here before [68](#0.8.0-changed-breaking) gave it one | `emit_error`, `emit_warning`, `emit_lexer_error`, `emit_unexpected_token`, `emit_skipped_region`, `enter_label`, `exit_label`, `cst_start`, `cst_finish`, `cst_mark`, `cst_start_at`, `emitter_bound_source`, `emitter_ref` |
| `ParseAttempt` | `into_option` |
| `Ident` | `parse_except`, `try_parse_except` |
| `InputContext`, `ParserContext` | `with_recursion_limiter` |
| `RecursionLimiter` | `unlimited` (an associated function — see the resolution rule below) |
| `SeparatedError` | `name`, `with_name`, `display_fmt` |
| `MissingToken` | `name`, `with_name` |
| `UnexpectedEnd` | `is_terminal`, `into_terminal` |
| `Unclosed` | `kind` |
| `Transaction` | `rollback_abandoning_points` |
| any parser-shaped type — one implementing `ParseInput` or `TryParseInput` | `labelled`, `traced`, `list_until`, `separated1_by`, `peek_then_head`, `opt` |

That is a finite question you can answer about your own code, and it is the one to answer.
**Whether the compiler then tells you depends on how close your signature is to ours — so do
not rely on it telling you, and do not rely on using the return value.** The remedy is UFCS:
`MyTrait::peek_kind(inp)` pins your method by name and is immune to all of this.

**The two forwarding rows are not the same disclosure as the four above them, and saying so is
the point.** The other rows name helpers tokora happened to grow; those two name 38 methods
that exist *because* [68](#0.8.0-changed-breaking) took `inp.emitter()` away, under the emitter's
own method names, on the two receivers a 0.7.3 grammar already held. A consumer who wrote
`fn emit_error(&mut self, ..)` on `InputRef` to shorten `inp.emitter().emit_error(..)` was
writing the most natural helper available in 0.7.3, and it now competes with a tokora method of
the same name, the same receiver and a compatible signature. If you have an extension trait over
`InputRef` or `ParseState` at all, read its method list against those two rows rather than
scanning for a name you remember choosing.

**Why there is no shortlist of "the silent ones".** An earlier draft of this entry named
four, measured. It was still wrong: the measurement covered one shape of consumer. Whether a
collision goes silent depends on how close the consumer's signature is to tokora's — a
property of code that does not exist yet — so a list of silent names is not a wrong answer,
it is not a well-formed one. The table above is the question we *can* answer for you.

#### The dangerous case is the one that reads as safest

**If you wrote the helper first**, with the same name and a compatible signature — because
you needed `peek_kind` or `head_satisfies` while tokora lacked them — then tokora's method
takes the call with **no error and no warning, even when you use the return value**.
Measured, byte-identical consumer source, base against this release:

```
base  CONSUMER-CALLS: 1     head  CONSUMER-CALLS: 0     zero diagnostics on both sides
```

reproduced for `peek_kind`, `head_satisfies` and `peek_head_map` with the result bound by `?`
at the caller. **A discarded return is not the precondition; a compatible signature is
enough.**

And the inversion that matters: **the closer your helper is to ours, the quieter the
substitution — and the more likely it also changes behaviour.** `peek_kind` and
`head_satisfies` deliberately have *different halt semantics* from the `peek_one`-based
helper a consumer would have written; that difference is why they exist (see the
terminal-stop entries below). So a near-identical replacement is the most dangerous case
here, not the safest.

Nothing in this release detects this for you. `ci/name_collision/` probes a consumer whose
signature *differs* from tokora's — the loud case — and reports argument-taking names as
`loud` on the strength of its own probe's arity. That limit is stated in
`ci/name_collision/README.md`, and its pass message spells the same limits out inline — that the
inherent-method and associated-function probes take no arguments, so a `loud` verdict on an
argument-taking name in those two categories comes from the probe's own arity, while the
trait-method probes pass real arguments and their rows are genuine — so a reader of a CI log
is not told "PASS" without being told what it excludes.
Three reproductions of the case it cannot generate are checked in beside it.

#### The one sub-case that is fully characterised

A consumer whose signature is *incompatible* with tokora's, calling with the return
**discarded**. There the outcome does depend on the return type. This is a sub-case, not the
boundary — it is here because it is the part the tooling covers:

| name | discarded value | reported as |
|---|---|---|
| `peek_kind` | `Result<…>` | ``warning: unused `Result` that must be used`` |
| `list_until` | `impl FnMut(…)` | ``warning: unused implementer of `FnMut` that must be used`` |
| `opt` | `impl FnMut(…)` | ``warning: unused implementer of `FnMut` that must be used`` |
| `into_option` | `Option<…>` | **nothing** — `Option` is not `#[must_use]` as a type |
| `labelled` · `traced` · `peek_then_head` | a plain adapter struct | **nothing** |

The discriminator there is whether the `unused_must_use` lint fires: a callable return is
covered by the lint's own rule for unused function-like values, an `Option` is not covered
at all, and a plain struct is not either. It is not "returns `Result`" and not
"is `#[must_use]`".

**There is deliberately no "full silent surface" list here.** An earlier draft gave one, of
fourteen names — and it omitted `parse_except` and `try_parse_except`, which are exactly the
two the paragraph above refuses to call safe. Any such list is the well-formedness problem
again. **The names to check are the exposure table at the top of this section** — it lists every
method and associated-function name this release adds **on a type that already existed in
0.7.3**, against the receiver each one lands on. Names on types this release itself introduces
are outside that question by construction, not by omission: you cannot have written an
extension method on a type you could not name. No count is restated here: a restated count is a
second inventory that can fall behind the first, and this one twice did. Which of those names go
silent for *your* code depends on your signatures, which is a thing only your code can answer.

What was measured, on rustc 1.87, 1.95 stable and 1.97-nightly:

- **Pick order, not inherent-ness, decides.** For `recv.m(..)` rustc walks the receiver's
  autoderef chain and, at each step, tries the receiver by value, then `&`, then `&mut`.
  Each is a *pick*. Within one pick an inherent candidate beats a trait candidate — but a
  tokora method taken **by value** sits at an earlier pick than your `&self` method on the
  same type and wins regardless, even against your *inherent* method. A method reached
  through a type parameter's bound (`fn f<T: MyTrait>(t: &T)`) buckets inherent at that
  pick, so generic code that names its combinator in a `where` clause is protected and
  concrete code is not.
- **How loud it is depends on your code's shape, not on the steal.** A fully inferable call
  site flips **silently** — compiles, no warning, runs the other method. A generic one
  leaves type variables unresolved and gets **`E0282`**. A concrete incompatible one gets
  **`E0308`**, or **`E0061`** on an arity mismatch, pointing into tokora's source. None of
  the three names the real cause.
- **A discarded return removes the last defence.** If a call ignores the result there is no
  type for the compiler to disagree about, and the steal is silent even where the same call
  with its return *used* is loud. Which names that reaches is the `unused_must_use` table
  earlier in this section: four report nothing, three report a warning, and the rest fail to compile — but
  only for a consumer whose signature is *incompatible* with ours, which is the sub-case
  that table describes.
- **The one breadcrumb.** If your method came from an extension trait you imported with a
  `use`, the steal strands that import and rustc emits **`warning: unused import`** naming
  the trait. An unused-import warning on a combinator trait after upgrading means one of
  its methods was re-resolved. If the trait is declared in the same file, you get nothing
  at all. The remedy either way is UFCS: `MyTrait::labelled(parser, "name")`.

**One entry here was measured after the rest of this section was written.** It concerns a
name this release adds, which the exposure table above already lists — what that table did
not carry is that this one is measured **silent**, and that is news a 0.7.3 consumer needs.

#### `RecursionLimiter::unlimited` — measured SILENT, and it is your call site to check

**You are exposed if you wrote an `unlimited()` associated function on `RecursionLimiter`.** That
type is public in **0.7.3**, so this is not hypothetical: any consumer holding one already had
somewhere to hang the name, and 0.8.0's
[`RecursionLimiter::unlimited`](https://docs.rs/tokora/latest/tokora/state/recursion_tracker/struct.RecursionLimiter.html#method.unlimited)
now competes for it by path.

Reproduced two-sided by `ci/name_collision/`, base `60f27a3` against this branch:

```text
SILENT  unlimited/discarded    base=witness=1  head=witness=0
```

Both sides compile, **neither emits any diagnostic**, and the two run different functions: yours
on 0.7.3, tokora's on 0.8.0 and later. `unlimited/used` is `loud` on the same probe, so a
discarded return is the whole of the difference — `RecursionLimiter` carries no `#[must_use]`, and
`unused_must_use` does not fire on a plain struct. **The remedy is UFCS**: `MyTrait::unlimited(..)`
or `<RecursionLimiter as MyExt>::unlimited()` pins your function by name and is immune to this.

`#[must_use]` was considered and rejected as the fix: it would not reach the case that matters —
a consumer who *assigns* the result, which the harness cannot generate and which stays silent
regardless — while firing on legitimate discards. Disclosure is the honest instrument here.

Two things about *why this arrives late*, both of which are the point rather than an excuse:

- 0.8.0's exposure table already lists `unlimited` under "you wrote it on `RecursionLimiter`", so
  the *name* was disclosed. What was not disclosed is that this one is **measured silent**, in a
  category the harness's README says a silent row "would be news" in — the first
  `inherent_assoc_fn` row ever to score one. The probe could not construct it at the time: #147
  introduced the owner and `gen_probe.py` had no template for it, so every row on that owner came
  back `FATAL` — an *incomplete* verdict, which is not a clean one. #148's Stage A added the
  templates and the `new-owner` verdict, and the finding fell out immediately.
- It is disclosed on **this** branch and could not wait. The probe's inventory is a two-sided
  delta: once this merges, `unlimited` exists on both sides, the row leaves every future plan, and
  the harness can never re-litigate it. A green run after that would mean "not probed", not "not
  colliding".

Recorded in `ci/name_collision/disclosed.txt`, whose fourth-and-fifth-row split now states plainly
that the earlier four ride a bounded receiver and **this one does not**: `RecursionLimiter` is a
concrete public struct with no bound to reject anybody.

#### `ParseState`'s zero-argument forwarders — four measured SILENT

**You are exposed if you wrote a method of your own on `ParseState`**, the view a `map_with` /
`and_then_with` / `validate_with` / `fold` callback is handed. That type is public in **0.7.3**
and until [68](#0.8.0-changed-breaking) it declared **no inherent item at all** — a callback
reached the emitter through `state.emitter()`, so `fn exit_label(&mut self)` on `ParseState`
was the natural way to shorten that, and it now competes with a tokora method of the same name
on the same receiver.

Reproduced two-sided by `ci/name_collision/`, base `7b289bc` against this branch, rustc 1.97.1:

```text
SILENT  cst_mark/discarded              base=witness=1  head=witness=0
SILENT  emitter_bound_source/discarded  base=witness=1  head=witness=0
SILENT  emitter_ref/discarded           base=witness=1  head=witness=0
SILENT  exit_label/discarded            base=witness=1  head=witness=0
```

Both sides compile, **neither emits any diagnostic**, and the two run different programs: yours
before, tokora's after. Each `used` row is `loud` on the same probe (`E0308`), so a **discarded
return** is the whole of the difference — and none of the four returns something
`unused_must_use` fires on: `()`, a plain `EventMark`, an `Option`, a shared reference. Same
discriminator as the `unused_must_use` table above — the lint, not a `#[must_use]` attribute.

**Read this as "the four the harness could reach", never as "the other nine are safe".** These
four are the whole of `ParseState`'s zero-argument set; the probe's consumer takes no arguments,
so for the nine forwarders that take one or two the head side fails on the probe's **own arity**
(`E0061`) and the row is filed `loud` — a verdict about the probe, not about the name. The
question that terminates is still the exposure table at the top of this section, and the remedy
is still UFCS: `MyTrait::exit_label(state)` pins your method by name.

Recorded in `ci/name_collision/disclosed.txt`. They are disclosed on **this** branch for the
reason `RecursionLimiter::unlimited` was: the probe's inventory is a two-sided delta, so once
this merges the names exist on both sides, the rows leave every future plan, and the harness can
never re-litigate them.

<a id="0.8.0-source-breaking-additions-that-fail-loudly"></a>

### Source-breaking additions that fail *loudly*

- **`E0034`, "multiple applicable items in scope"**, when one of the **14** new defaulted method
  names on a trait you already had meets a same-named method from another trait **at the same
  pick**, with that trait in scope: `labelled`, `traced`, `list_until`, `separated1_by`,
  `peek_then_head`, `try_delimited`, `try_delimited_by_parens`, `try_delimited_by_braces`,
  `try_delimited_by_brackets` and `try_delimited_by_angles` on `ParseInput`; `opt` on
  `TryParseInput`; `bound_source` and `commit_lexer_error` on `Emitter`; `observe_separator` on
  `SeparatorHandler`.
  (`CstText::cst_text` and `MaybeTerminal::is_terminal` are the release's other two new trait
  methods and are not in that thirteen: their traits are new, so they compete only once you
  import them.) Blanket-ness is irrelevant — a trait with exactly one impl for one concrete
  input type collides identically. rustc prints its own disambiguation suggestions; the fix
  is UFCS.

  **`commit_lexer_error` is the one name in that list the probe cannot construct, and saying
  so is the point.** It takes `&mut self`, and the harness's three trait-method spellings
  declare `self` or `&self` — both **earlier** picks on a non-deref receiver, so the
  consumer's item wins on both sides and tokora's is never a candidate. The `E0034` above
  therefore rests on the same-pick rule rather than on a measurement, and it applies to the
  consumer who declares `&mut self`, which is the shape that competes. Recorded, with the
  receiver walk, in `ci/name_collision/no_collision.txt` rather than left to look like a
  clean run.

- **Associated functions resolve by a different rule, and it is not the receiver walk.**
  `Ident::parse_except` and `Ident::try_parse_except` are inherent **associated functions**.
  A path call `Type::name(..)` has no receiver, so no autoderef and no autoref happen;
  inherent associated items simply beat trait ones. A consumer trait that supplied either
  name loses it.

  **These two are not more strongly protected than the methods above, and an earlier draft
  of this entry said they were.** It claimed every collision shape here is loud, on the
  strength of the probe — but that probe declares a zero-argument consumer against a
  two-argument function, so its `loud` verdict comes from its own arity and measures
  nothing about the name. Treat these exactly like the receiver methods: if you wrote
  `parse_except` on `Ident`, check the call site and do not rely on a diagnostic.

  What *is* established, for the narrow sub-case of an incompatible signature with the
  return **discarded**: both functions return `Result`, which is `#[must_use]`, so a
  discarded result warns.

  **Unsupported is not the same as disproven, and the difference is the whole finding.**
  Two attempts to build a silently-stealing consumer for `parse_except` produced a compile
  error instead; a different reviewer, with a different signature, produced a silent steal.
  Neither outcome settles it, because whether a collision is reported depends on how close
  the consumer's signature is to ours — and that is a property of code that does not exist
  yet. So this entry does not claim these two names are safe, and does not claim they are
  unsafe. It claims the earlier assertion of safety had nothing behind it.

- **Ambiguity between two glob imports**, for the **53** new glob-reachable names. Twenty of
  them were enumerated by path during the campaign and probed, and they are the list below;
  the other 33 are the new public types, traits, type aliases and functions this release adds,
  every one of them an *item* name, so the item row of the table below governs them too — they
  are under [Added](#0.8.0-added) rather than repeated here. The twenty: `Pinned`,
  `pinned`, `WhileHead`, `WhileKind`, `while_head`, `while_kind`, `select`, `try_select`,
  `attempt`, `syntax_kinds` via `tokora`; `EmitterView` via `tokora` **and**
  `tokora::emitter`; `dispatch_take`, `try_dispatch_take` via
  `tokora::parser`; `kinds`, `Cst`, `parse_lossless` and `parse_lossless_partial` via
  `tokora::cst`; `RecursionLimitReached` and
  `NonAssociativeChain` via `tokora::error`; and `Descent` via `tokora::input`. The two new
  modules, `tokora::cst::kinds` and `tokora::dialect`, each open a glob namespace of their own.
  `select!` against `tokio::select!` or
  `futures::select!` is the real-world instance — and the four macro names are the whole of the
  macro row below, so the item row governs every other new name in this release, enumerated here
  or not. The four
  [68](#0.8.0-changed-breaking) adds are all *item* names; they were classified by kind from the
  branch diff rather than
  re-measured on the three toolchains, which is the same treatment items
  [41–46](#0.8.0-changed-breaking)'s three had. **The diagnostic and its remedy differ by what
  kind of name collides, and — for macros only — by toolchain:**

  | Colliding name | 1.87 | 1.95 stable | 1.97 nightly |
  |---|---|---|---|
  | item (`Pinned`, `while_head`, …) | hard `error[E0659]` | hard `error[E0659]` | hard `error[E0659]` |
  | macro (`select`, `try_select`, `attempt`, `syntax_kinds`) | hard `error[E0659]` | `ambiguous_glob_imports` **warning** | `ambiguous_glob_imports` **error** |

  `#[allow(ambiguous_glob_imports)]` **does not suppress the item rows on any toolchain**,
  and does not suppress the macro row on 1.87 either. For an item name there is exactly one
  fix: an explicit `use` naming the one you meant.

**Why the names were not changed instead.** 0.8.0 is a breaking release and these are the
right names. One name *was* removed rather than disclosed — see `pinned` under
[Added](#0.8.0-added) — because it was the only addition that planted a candidate on every
`Sized` type in your program, and the argument for accepting that was the loudness
guarantee the turbofish spelling refuted. A source census of the known consumer, run
during development, found zero declarations of any of these names and zero
`use tokora[::…]::*` glob imports. **That census is not re-runnable from this repository —
its script is not shipped here — so treat it as a development note rather than a check you
can repeat.** It also saw one consumer at one commit. The disclosure above is the control;
the census was never more than corroboration.

<a id="0.8.0-changed-breaking"></a>

### Changed (breaking)

Numbered once across the whole release. The groups are ordered by **how a break announces itself**: the ones that do not fail to compile come first, because nothing in your build will point at them.

*Repetition drivers' end-state accounting.* Six deliberate behaviour changes, all in the
repetition drivers' end-state accounting. No API is removed or renamed.

1. **A properly closed separated + delimited list now reports its count bounds and separator
   policy.** `…separated_by_*().delimited::<D>()` left its element loop through the mid-scan
   closer without running the end-state pass, so on every well-formed, properly closed list the
   whole of `at_least` / `at_most` / `bounded` and the leading/trailing separator policy was
   dead code — the sibling `separated_*_while` driver ran it on the identical path. Under a
   recovering emitter such a list now records the diagnostic it always should have; under a
   fail-fast emitter a bounds- or policy-violating closed list is now an `Err` where it used to
   be a silent `Ok`.

   — *(R7, #117)*

2. **`TooMany` is emitted once per construct, with a count that exceeds the limit it names.**
   `nums()` is now `limit() + 1` at every emission site. It used to be either the running count
   (so the same four-element history yielded a different payload from each builder) or, from
   the mid-loop hook, the limit itself — a "found 2 … exceeds … 2" that contradicted itself.
   The duplicate emission (mid-loop hook plus a delegating end check) is gone.

   — *(R7, #117)*

3. **`FullContainer` is emitted once per construct**, on the whole-construct span in every
   driver, with a `nums()` that includes the refused element. A container that refuses one push
   refuses every later one, so the old per-dropped-element re-emission produced a count that
   climbed past the capacity it named.

   — *(R7, #117)*

4. **Count bounds now judge the elements the driver parsed, not the ones the container
   stored.** In the separated drivers a bounded container used to turn a satisfied `at_least`
   into a spurious `TooFew`, and — worse — to swallow a violated `at_most` entirely by clamping
   the count the check reads. Only bounded-capacity containers are affected; with `Vec`,
   `VecDeque`, `SmallVec` and `TinyVec` the stored and parsed counts are always equal.

   — *(R7, #117)*

5. **`SeparatorHandler::on_separator` now receives every separator in source order.** It used
   to receive the exact complement of its documented contract — only the leading separator and
   the duplicates in a run, never the happy-path or trailing ones. A container that ignores
   separators can opt out with the new defaulted `SeparatorHandler::OBSERVES_SEPARATORS`
   associated const, which suppresses both the call and the clone that feeds it; every
   container in this crate does. Adding the const makes `SeparatorHandler` no longer
   object-safe — `dyn SeparatorHandler` no longer compiles. Nothing in this crate used it.

   — *(R7, #117)*

6. **The delimited separated drivers return the whole construct's span**, opener through
   closer, on every success exit. Some exits previously returned a span measured from the first
   element instead, so the returned span excluded the opener and depended on which exit the
   parse took.

   — *(R7, #117)*

*The input layer's unwind edges.* Nothing that can unwind runs between taking a token out of the
stream and recording where it went. Every scan, guard and emitter edge that previously had caller
code in that gap now settles on the panic path as well as the return path, and the rollback
protocol has one owner instead of two.

7. **`Cache::rewind` is removed.** Rollback was implementable in two places — the cache
   could rewind itself, and the input drives the same rollback through its lineage — so a
   cache that took the first route and an input that took the second disagreed about what
   the stream held. There is now one owner. Implementors of `Cache` delete the method; the
   four in-tree implementations did, and no caller loses a capability, because every public
   rollback path already went through the input.

   — *(R9)*

8. **`Cache`'s non-panicking restore-path law now names `push_front`.** The clause previously
   listed the reading and removing operations the crate calls while restoring — `pop_front`,
   `pop_back`, `clear`, `front`, `front_span`, `len` — and omitted the one *writing* operation,
   which is exactly the one the new unwind edge in item 9 depends on. An implementation whose
   `push_front` can panic was vacuously conforming before and is not now.

   The rider is stated rather than implied: a `push_front` that panics is the single case the
   crate **cannot** make whole, because it has already taken the token by value and nothing can
   un-take it. Every other restore-path operation is recoverable.

   — *(R9)*

9. **A panic inside caller code no longer corrupts the stream.** `skip_until` pops a token
   out of durable state — the parked slot, or the cache front — and then runs caller code
   over it: the predicate, the expected-tokens closure, `L::State: Clone`, the lexer
   itself. Any of those can panic, and a panic through one used to be an exit that no
   put-back and no settle ever saw. The in-flight token was dropped with an unowned local,
   the skipped prefix stayed behind a frontier that was never committed, and a rewinding
   mode's entry mark was neither rewound nor released. **With a warm cache that lost tokens
   outright.** A `Verbose` record could likewise tear if the payload's own allocation
   panicked mid-push.

   The unwind edge is split by **disposition**, and each half mirrors an exit the scan
   already owns. Committing modes (`SyncTo`, `SkipWhile`) behave as their fatal exit does —
   commit the diagnosed prefix, put the in-flight token back — so a host that catches and
   retries resumes *after* the diagnosed prefix with no duplicate reports. Rewinding modes
   (`SyncThrough`, `SyncBalanced`) restore the full pre-call state and rewind the mark **when
   the scan is abandoned mid-flight**.

   Disposition follows the **exit, not the mode**: once a stop has been decided, an
   interrupted stop keeps the diagnosed prefix for every mode. `SyncBalanced` is where that
   distinction is load-bearing — it rewinds at end of input and keeps on a stop — so the
   guard reads which exit it was interrupted in rather than which mode it is running. A uniform clear was tried and
   measured doing real harm on the committing path — it discarded pre-entry cache entries,
   the re-lex re-burned a shared limit budget, and a poison boundary latched at a position
   the original lineage never reached.

   This is a behaviour change only for a host that catches a panic across a parse and keeps
   using the input. Callers that let a panic propagate see no difference.

   — *(R9)*

10. **The committed position is written as one step, everywhere.** The position is a *pair* —
    the span, and the lexer state that produced it — and many sites wrote it in two steps with
    caller code in between. At end of input, `SyncTo::on_eof` advanced the span and then called
    `lexer.into_state()`; a panic there left the span advanced with the entry state still paired
    to it, so a host that caught and resumed lexed from the new offset under a state that had
    seen nothing. Reachable through `sync_to`, `skip_while` and `padded`, and only for
    **stateful** lexers — the population least likely to notice.

    **Advancing the span without supplying the state is no longer expressible.** The span-only
    setters are gone — once every caller had to pass the state, the compiler reported them as
    dead code — and the internal token settle carries the state to a single funnel that
    evaluates every fallible step first, writes both halves as infallible moves, and drops the
    replaced values afterwards. A `Drop` is caller code; dropping in place would run it between
    the halves, which is the tear itself. Where a restore has more to do than the position, the
    funnel hands the replaced pair back so the caller drops it only after the watermark, the
    poison latch and the emitter mark are restored too.

    **Both halves are installed before either replaced value is dropped**, at every site that
    writes one. That ordering is the property a future change has to preserve, and the reason
    is easy to miss: an assignment installs its new value and *then* drops the old one, so
    `span = …; state = …;` lets the replaced span's destructor run **between** the two writes.
    If it unwinds there, the second write never happens and one token's span is published
    beside the previous token's state — off by exactly one, silently, and only for spans or
    states that have a destructor at all.

    This is internal surgery: `Emitter::commit_token`'s signature is unchanged, so emitter
    implementations need no edit. The one implementor-visible consequence is an ordering change
    — the `commit_token` observer hook now runs **after** the position is published and
    **before** the replaced pair is dropped, so a panicking destructor cannot swallow the
    notification.
    Every caller reaches it having already taken the token off the front of the stream, so
    notifying first would leave the committed position naming a token the stream no longer
    holds, and a host that caught the observer's panic would resume past a token it never saw.
    An observer that panics now leaves the input consistent, with only the notification missing.

    — *(R9)*

11. **`Lexer` gains a clause: the settle path's operations must not panic.** This is a real new
    constraint on `Span` and `Offset` implementations, not a footnote, and it is the one window
    in the consume path that **cannot** be closed by reordering.

    The input layer settles a consumed token by computing the committed position **from that
    token's own span**. The span does not exist until the token has been popped, so those steps
    necessarily run after the token has left the stream and before the position has moved. The
    operations are `Source::len`; `Span::end_ref` and the `Ord` on `Offset` that decide whether
    a clamp is needed; `Span::clone` on the common path; and, only when the span is clamped to
    the source, `Span::start_ref` plus `Offset::clone` plus `Span::new`. **None of them may
    panic.**

    Two operations that were in this window during development are **not** in it and are
    deliberately absent from that list: the `Drop` of the span and state a commit replaces, and
    the `Drop` of the `Source::len` temporary. Both were moved out — the replaced pair is handed
    back to the caller and dies after the settle, and the length temporary now rides out with
    it. Nothing above is removable for the same reason, stated once: **every one of them is
    computed from the token's own span, which does not exist until the token has been taken.**

    The posture matches `Emitter::release`/`rewind` and the `Cache` restore-path operations, and
    for the same reason: the operation runs where nothing else can repair it.

    **The bounds are unchanged: `Lexer::Span` still requires `Clone`, not `Copy`.** A span that
    allocates, refcounts or carries a `Drop` is still welcome — an `Arc`-carrying file id clones
    by bumping a refcount, which cannot panic, and satisfies this clause fine. `Copy` would force
    the clause for free, and was rejected precisely because it would ban working code to close a
    window that a non-panicking `Clone` already closes. The in-tree types happen to satisfy it
    trivially (`SimpleSpan` is `Copy`; `usize` offsets neither allocate nor drop), which is why
    the default configuration pays nothing.

    Like the two clauses it matches, this is an obligation the crate **states and cannot check** —
    Rust has no "this impl does not panic" bound. A span whose `Clone` can allocate and abort
    under memory pressure will still lose a token, and nothing catches it at compile time.

    **What you get in return is a statable guarantee:** if your span's clone, construction,
    comparison and drop do not panic, a consumed token is never lost.

    Two alternatives were measured and rejected. Cloning the span before the pop costs an extra
    `front()` read on `next`, the hottest path in the crate; passing the span by value only
    relocates the clone — it helps `next`, which destructures, and hurts `consume_cached_one`,
    which keeps the `Spanned`.

    — *(R9)*

12. **An `Incomplete` scan exit leaves no trace.** A scan that ran out of input mid-decision
    used to commit the frontier it had reached, so a caller that topped up the source and
    retried resumed at a position the first attempt had already half-consumed. It now exits
    as it entered.

    — *(R9)*

*CST sink construction and materialization.* A CST sink is bound to its source when it is built,
not when it is finished, and the materialization walk no longer rescans.

13. **`Sink::new` takes the source and a `CstProfile`; `finish` and `finish_partial` no
    longer take a source.** The source is bound once, at construction, and the three loose
    `u16` parameters (`mapper`, `error_kind`, `gap_kind`) move into `CstProfile` alongside
    the new `KindValidator`.

    **What this buys, stated exactly.** There were two chances to hand the sink the wrong
    buffer: at construction and again at `finish`. The second is gone — you can no longer
    materialize against a source the sink was not built with. The first remains open, and it
    is one witness of the limitation below.

    ```rust
    // before
    let sink = Sink::new(emitter, map_kind, ERROR, GAP);
    let (green, emitter) = sink.finish(ROOT, source);

    // after
    let sink = Sink::new(source, emitter, CstProfile::new(map_kind, ERROR, GAP));
    let (green, emitter) = sink.finish(ROOT);
    ```

    — *(R8, #123)*

14. **An out-of-language kind now panics at the door, in every build.** A dialect mapper that
    returns a kind outside its own kind space — or the reserved tombstone `u16::MAX` — is
    refused by an `assert!` at the emit site (`cst_start`, `cst_start_at`, and the shared
    `record_token` body behind both token doors, plus `CstProfile::new` for the two
    synthesized kinds). Previously the same input reached `finish` and came back as
    `Err(FinishError::InvalidDialectKind)`.

    **This is a behaviour change for a caller that relied on the error.** It is a panic
    because the condition is a dialect bug, not an input condition: the mapper is the
    dialect's own code, no parse input can provoke it, and refusing at the cause reports it
    one materialization earlier than the error did.

    The honest limit, which is also stated at the site: the *per-event* validator inside
    `finish` is `cfg!(debug_assertions)`-gated, because keeping it in every build costs a
    measured **+4.4%** on ordinary materialization — an indirect call per event inside a tight
    builder loop. Every route reachable from outside this crate goes through a door that
    validates in every build, so for external callers the wall is absolute. The one route with
    no door is `push_raw_event_for_tests`, which is `pub(crate)`. So: a **release** build whose
    event log was assembled by raw in-crate injection can materialize an out-of-language kind.
    Debug-assertions test runs, and the CI profiles that run this cell, refuse it;
    release-profile test runs do not exercise that wall. `ReservedKind` is not gated — the
    tombstone band is a plain comparison and was a release wall before this round.

    **That +4.4% is measured on the shipped fused walk, and it withdraws a +8.3%.** The
    withdrawn figure was taken on the two-pass gather+walk shape item 51 superseded, where
    these three calls sat in the *gather* pass rather than in the builder loop. The
    remeasurement is the shipped tree against the same tree with `cfg!(debug_assertions) &&`
    deleted from exactly those three sites and nothing else: the inlined `materialize` carries
    one indirect branch in the shipped build — the once-per-materialization root check — and
    four in the ungated one, so the per-event call is really there and really indirect.
    `finish_clean` (8 192 tokens) reads **+4.4%**, the median of sixteen interleaved paired
    rounds, positive in all sixteen and spread +2.1% to +6.9%, against a null control of two
    byte-identical builds of one source read as that same per-round statistic: −1.15% to +1.13%
    about a median of −0.08%, so it is wider than the aggregate-median floor the **Performance**
    section quotes and is the one this comparison is scored against. The two populations do not
    overlap. `finish_error_dense` reads +2.0%, `finish_wrap_heavy` +3.3%. The same deletion on
    the *two-pass* shape reads **+1.1%** — a median inside the control's own span, populations
    overlapping, only eleven of sixteen rounds above their own round's control — so 8.3% is
    reproduced on neither shape, and in absolute terms the fold made these three checks dearer
    rather than cheaper: 0.07 ns per call in the gather pass against 0.37 ns in the builder
    loop. One code layout on one toolchain, so read it as low single digits rather than as a
    constant. The trade the gate buys is smaller than this entry claimed. It is still real, and
    clear of the noise floor, and the gate stays.

    — *(R8, #123; the per-event cost remeasured against the shipped fold)*

15. **`FinishError` gains `InvalidDiagnosticSpan`, `MismatchedFinish`, `NonUtf8Source` and
    `InvalidDialectKind`, and `CstEmitter::cst_finish` takes the kind it intends to close.**
    The sink now names the node it closed, so a mismatched finish reports which frame it
    failed against instead of producing a misparented tree — and the identity it compares
    against has to come from somewhere, so `cst_finish` gained a `kind: u16` parameter. It is
    a **defaulted** trait method, so an implementor that does not override it is unaffected;
    every *caller* and every overriding implementor passes the kind it passed to the matching
    `cst_start` / `cst_start_at`.

    **Migration:** `emitter.cst_finish()` becomes `emitter.cst_finish(KIND)`, with `KIND` the
    value the matching open used. Passing a different kind is what
    `FinishError::MismatchedFinish` reports.

    — *(R8, #123)*

*Terminal stops, collection drivers and session points.* A scanner that stops on a resource
limit or a poison boundary is not an end of input, and a driver that cannot make progress is
not a driver that succeeded. Four rounds of repair share one shape: a condition that used to
be reported as *absence* is now reported as *failure*, and the types say so.

16. **A terminal scanner stop is surfaced as an error, never as a decline.** A resource-limit
    trip or a latched poison boundary used to reach a caller as an ordinary end-of-input
    decline, so recovery treated it as "nothing here" and retried against a scanner that had
    already given up. `UnexpectedEnd` now carries a **terminal** flag, raised through
    `into_terminal()` and read back through `is_terminal()`, and the new public trait
    `error::MaybeTerminal` is the channel recovery keys off to re-raise the stop instead of
    recovering it.

    **This is a bound addition on the error type.** `From<UnexpectedEot<L::Offset, Lang>>` is
    now required on the committed peek and `Expect` surfaces, and a downstream error type must
    implement `MaybeTerminal` — an empty `impl MaybeTerminal for MyError {}` is enough unless
    the type can itself carry a terminal stop, in which case it delegates. This release ships a
    **second** terminal carrier alongside the flag: `RecursionLimitReached` (item 46), which is
    terminal for every value, so a type that stores both delegates to both. A **third** terminal
    source ships in this same release and is easy to miss precisely because it is not a
    `MaybeTerminal` implementor at all: `SessionRefusal`. `PartialSession::parse` converts it
    through the consumer's `From` and then asserts the result is terminal, unconditionally — so a
    `Refused(SessionRefusal)` arm left at `false` panics a release build rather than being spent
    (see *A session's budget guard now holds in release builds*, under Fixed). Those three are the
    values this crate **builds and marks**, which is not the same as a closed set of terminal
    conditions: when a *rejecting* emitter refuses a scanner trip's diagnostic, the emitter's `Err`
    is what propagates, built from your lexer's error value through
    `From<<L::Token as Token>::Error>` with no marker on it — the same trip, unmarked, reaching you
    through a different carrier. The trait's own doc carries the three-source table, that path, and
    the rule for an arm the table does not name, including terminal carriers of your own that this
    crate cannot enumerate.
    The try-leaf surface gained the `*_or_stop` shape
    (`InputRef::next_or_stop`, `try_expect_map_or_stop`, `peek_with_emitter_terminal`) so a leaf
    can report the stop without inventing a decline.

    **Derived `Debug`, `Eq` and `Hash` on `UnexpectedEnd` moved with the field.** The struct
    derives all three, so the new private `terminal: bool` appears in every `{:?}` render and
    participates in equality and hashing. A consumer with frozen renders re-blesses; a consumer
    comparing two errors that differ only in terminal status now sees them as unequal, which is
    the point. *This break shipped four rounds ago and was never disclosed anywhere until this
    entry.*

    — *(R1, #111)*

17. **`From<UnexpectedEot<L::Offset, Lang>>` is required at thirteen further sites.** The
    collection drivers stopped masking a terminal stop as a successful end, and stopped
    spinning on zero progress; both exits have to be expressible as an error, so
    `Repeated::parse`, the plain fold wings (`Fold`, `TryFold`, `TryFoldWith`, `RFold`) and the
    `Repeated` caller impls carry the conversion. A production that already names
    `ComposableParseContext` (or `FromTokenErrors`) gets this for free; one that spells its
    conversions individually adds this one.

    — *(R2, #112)*

18. **A `Sink`'s inner emitter must implement `ValueKeyedEmitter` — `Sink<Sink<E>>` is now a
    compile error.** A sink keys its bookkeeping by emitter mark; nesting one inside another
    made two independent mark spaces claim the same keys, so an abandon on the outer sink
    settled captures the inner one still owned. `ValueKeyedEmitter` is a new public marker
    trait with no members, implemented by every diagnostics emitter and deliberately not by
    `Sink`, which is what makes the nesting unrepresentable rather than documented against.

    — *(R3, #113)*

19. **`begin_point` returns a `#[must_use] SessionPointId`, and `commit_point` /
    `rollback_point` take it.** The three were argument-less, so nothing tied a settle to the
    point it settled and an out-of-order or duplicated settle silently corrupted the pin set.
    The id is branded to the handle and to the point, and four misuse conditions now panic by
    name: `no live session point`, `session point settled out of order`, `stale session point`,
    `foreign session point`.

    **Migration:** bind the return value — `let p = inp.begin_point();` — and pass it back:
    `inp.commit_point(p)` / `inp.rollback_point(p)`. Points settle newest-first.

    — *(R3, #113)*

20. **The two separator/token error carriers gained a name channel, widening
    `into_components` on both.** `MissingToken::into_components` returns a 4-tuple
    (`offset`, `expected`, `message`, **`name`**) where it returned a 3-tuple;
    `SeparatedError::into_components` returns a 3-tuple (`position`, `token`, **`name`**) where
    it returned a pair. Both types gain `with_name` and `name()`, and both derive `Eq` and
    `Hash` over the new field, so two otherwise-identical errors carrying different separator
    names no longer compare equal.

    **Migration:** add a binding for the trailing element at every `into_components`
    destructuring; `..` is not available on a tuple pattern, so this is a mechanical edit the
    compiler points at.

    — *(R4, #114)*

*The Pratt driver.* The Pratt driver ends an expression when the RHS channel says so, not when the
input position looks finished — and a report that consumes nothing can no longer be folded.

21. **`PrattRHS` gains an `End` variant.** A grammar says the expression is over on the
    same channel it uses to report operators, instead of relying on a sentinel operator
    below the minimum power. Exhaustive matches on `PrattRHS` must add an arm; the
    compile error is deliberate, which is why the enum is not `#[non_exhaustive]` — a
    wildcard arm is exactly the silent end-swallowing this change removes.

    — *(R6, #120)*

22. **The `is_eoi` loop gates are gone.** They ended the expression on a *position*,
    which a lookahead could move: a widening peek made a typed parse of `1+2` return
    `1`. The RHS channel now decides.

    — *(R6, #120)*

23. **Recursion floors are explicit.** `PrattFloor` replaces the arithmetic that
    reconstructed a floor from a power. Right-associative operators recurse at their own
    power (an off-by-one that made a right-associative pin yield 14 where 10 is correct);
    left and non-associative recurse strictly above it, which also removes the wrong
    parses the old code produced at `Power::MAX`, where the arithmetic saturated instead
    of separating. The token flavour carries its true left power rather than
    reconstructing it.

    — *(R6, #120)*

24. **`PrattPower::next` and `prev` are removed.** They were the arithmetic that made
    the floor bugs expressible; with them gone, reintroducing driver-side power
    arithmetic is a compile error rather than a review comment. This breaks callers, not
    only implementors, and no lint would have flagged them as unused — a required trait
    method warns nowhere. Note also that every in-repo implementation was non-saturating
    despite the trait's own documentation asking for saturation, so each carried a latent
    debug overflow panic.

    — *(R6, #120)*

25. **A report that consumes nothing is refused, terminally.** The typed driver checks
    committed consumption at the report boundary — after the floor and rejection logic,
    before any classify, fold, wrap, or recursion — and on a stall rolls the cycle back
    and returns a terminal `UnexpectedEoLhs` or `UnexpectedEoRhs`. Previously a
    zero-consumption report could be folded: a prefix reported after a peek recursed at
    the same position until the stack overflowed, and a zero-width infix produced
    `Ok(6)` from `1 2 3` with two phantom folds and no diagnostic on any channel.

    This adds `From<UnexpectedEoLhs<…>>` and `From<UnexpectedEoRhs<…>>` to the typed
    driver's error requirements. They are the engine's terminal exits for a
    `parse_lhs`/`parse_rhs` contract violation — ordinary operand exhaustion is still
    `UnexpectedEot` and is unchanged.

    — *(R6, #120)*

26. **The trace channel is more verbose.** With the position gate gone, the RHS channel
    is consulted at exhaustion, so traces gain the consultations the gate used to skip.
    Parse results and non-trace output are unchanged.

    — *(R6, #120)*

The token driver needs none of this: `PrattToken::try_pratt_rhs` and `try_pratt_lhs`
take only `&self` with no `InputRef`, so a report there cannot be made without
consuming, and no token fold receives an `InputRef`. That asymmetry is why the guard
lives in one driver and not the other.

*One bound per production, and the language fences.* The bound a grammar production writes is now
**one line**. Where a production previously restated every conversion its callees might need, it
names a single context bundle and the compiler elaborates the rest.

27. **`ParseCtx` is renamed `ComposableParseContext`** and completed. It now carries
    `FromTokenErrors` — the five token-level conversions as one supertrait — so the nested
    associated-type bound elaborates to callers. `ParseContext` is unchanged. The rename
    touches 59 occurrences across 14 files; the crate-root re-export set gained and lost
    nothing.

    — *(W-api-A, #118)*

28. **`FromUnclosed` replaces the per-delimiter `From<Unclosed<D, …>>` family.** One bound
    covers every delimiter pair, discriminated at runtime with a mandatory catch-all arm.
    `UnclosedEmitter::emit_unclosed`'s method bound moves with it. Type-level per-pair `From`
    impls still work alongside the umbrella, and remain the documented way to discriminate at
    the type level.

    **Migration:** delete the per-delimiter `From<Unclosed<…>>` bounds from your where-clauses
    — a residual one now fails as a bare `E0277` with no note, because `From` is a foreign
    trait we cannot annotate.

    **The discriminator is `Unclosed::kind`, not the display name.** Two further breaking
    changes carry it:

    - **`Delimiter` gains a required `const KIND: DelimiterKind`** (new, exported from
      `tokora::delimiter`). No default: a defaulted identity is one the author never chose.
    - **`Unclosed::new` and `Unclosed::of` take the kind** between the span and the name, and
      `Unclosed::kind()` reads it back. The four typed constructors (`Unclosed::paren` and
      siblings) are unchanged.

    Routing on `Unclosed::name_ref` was unsound as a dispatch key and is no longer documented
    as one. `name` is a display string with no uniqueness contract — `"[]"` is the correct name
    for *any* bracket-shaped pair — so a consumer who defined a custom pair and named it
    correctly had it silently absorbed by the built-in `Bracket` arm, reporting a character the
    source never contained. A pair tokora does not define should declare `DelimiterKind::Custom`,
    a variant that can never equal a built-in.

    **The accident is unrepresentable: `DelimiterKind`'s four built-in variants are themselves
    `#[non_exhaustive]`.** Only tokora can *write* one. `DelimiterKind::Bracket` in another
    crate is a privacy error (`E0603`) — as a `Delimiter::KIND` declaration, as the `kind`
    argument to `Unclosed::new`/`of`, anywhere — so the author who reaches for `Bracket`
    because `"[]"` was the right name for their pair no longer compiles. Matching is untouched
    at a cost of one token: **an arm outside tokora is written `DelimiterKind::Bracket { .. }`**,
    the pattern form `#[non_exhaustive]` on a variant leaves open. A `compile_fail,E0603`
    doctest on `DelimiterKind` pins it, paired with a positive control differing in one token.

    **That fences the spelling, not the value, and it is not a provenance check.** A built-in
    kind is still obtainable by a crate that is not tokora, three ways: by projecting
    `<tokora::punct::Bracket<…> as Delimiter<…>>::KIND`, since the const is public and its type
    unconstrained; by reading `Unclosed::kind` and passing it back to `Unclosed::new`/`of`; or
    from `Unclosed::bracket`/`bracket_of` and their `paren`/`brace`/`angle` siblings, which mint
    a built-in-kinded error in one public call and need no `Delimiter` impl at all. The first
    and third name tokora's own pair; the second does not — a generic adapter forwarding an
    error copies whatever kind it was handed. So a `DelimiterKind::Bracket { .. }` arm firing
    means dispatch was not keyed on a display string — not that the caller chose the kind, and
    not that the diagnostic came from tokora. All three routes are pinned
    green in `tests/unclosed_kind_dispatch.rs`, asserting the forgery succeeds, so closing one
    later breaks a line rather than making this entry quietly wrong.

    The residue is accepted deliberately. Closing the projection alone is possible — a
    `#[doc(hidden)]` provided method with a parameter type unnameable outside tokora does it,
    while a sealed supertrait plus a blanket impl over a public key trait does not compile at
    all (`E0119` against the `&D` forwarding impl) — but it leaves the other two routes open and
    buys no stronger claim. Closing all three means making the eight typed constructors
    crate-private and reshaping `new`/`of` so they cannot take a kind, which makes
    `Unclosed<char>` unconstructible outside tokora. That is refused; `DelimiterKind`'s docs
    carry the full reasoning.

    What this does **not** fix, also stated at the type: two custom pairs in one crate can still
    both declare `Custom("mine")`; and a pair the arm set forgot still lands in the catch-all —
    over a generic `D` the compile-time obligation is gone for good, and `#[non_exhaustive]`
    keeps the catch-all mandatory.

    **Migration:** replace `match err.name_ref()` with `match err.kind()` and the string
    literals with `DelimiterKind::` variants — `DelimiterKind::Bracket { .. }` for a built-in,
    braces included; add `const KIND` to any custom `Delimiter` impl —
    `DelimiterKind::Custom("your-key")`, the only variant your crate can name; add the kind
    argument to any direct `Unclosed::new`/`of` call, or reach for
    `Unclosed::bracket`/`bracket_of` and their siblings where the pair is one of tokora's.
    `name_ref` is unchanged and remains correct for rendering.

    — *(W-api-A, #118 / #121)*

29. **The `_of` language twins are gone — 167 public items removed.** Every `X::parse_of` is
    now `X::parse`, inferring `Lang` from the input. This covers 148 punctuator methods
    (74 markers × 2), `Keyword` ×8, `Ident` ×2, `Any` ×3, six free functions, and two more per
    type generated by `keyword!`. **`list_of` is renamed `list`** — it had no bare twin, so the
    suffix carried no meaning.

    *Not* collapsed, because `Lang` is genuinely unrecoverable there: `ParserContext::of` and
    `with_cache_options_of` (an emitter does not determine its own language brand), the
    argument-less constructors, and every error constructor `_of` (their `Lang` is a phantom
    brand reached only through a `From` obligation). The `one_of` family is unrelated — it
    means "one of these", not "of this language".

    **Migration:** drop the `_of` suffix. A call site that already spelled its generics gains
    one type argument, `Lang`, in last position; `_` is almost always sufficient.

    — *(W-api-A, #118)*

30. **The fluent `separated_by_*` family works for branded grammars.** It previously failed on
    `.collect()` with an error pointing frames away from its cause, because the generated
    methods hard-coded the bare marker `Comma<(), (), ()>`. They now instantiate the separator
    at the caller's language — `Comma<(), (), Lang>` — so the return type of every
    `separated_by_*` method gains that argument.

    The marker's own brand and the context language remain the **same** parameter on the
    `Punctuator` impls: a marker branded for one grammar is not a punctuator of another.
    Widening the impl is a second route to the same fix and is deliberately not taken — it
    would let a separator copy-pasted out of a sibling dialect type-check silently. The fence
    is pinned by a `compile_fail,E0277` doctest on `Punctuator`, paired with a positive
    control differing in one token.

    — *(W-api-A, #122)*

31. **`Token::Kind: 'static` is hoisted to the trait**, deleting five projection restatements.
    Four superficially similar bounds remain: they constrain the dispatch structs' own free
    `Kind` parameter, which the hoist does not reach.

    — *(W-api-A, #118)*

32. **`Parser` loses its `Error` phantom parameter** and gains value-driven entry points
    (`parse`, `parse_with`, `parse_with_state`); its constructors collapse from eight to four.

    — *(W-api-A, #118)*

33. **The delimiter side gets item 30's fix.** `Delimiter` and `TypedDelimiter`'s blanket impls
    for `Paren`, `Brace`, `Bracket` and `Angle` carried a marker-language parameter independent
    of the context language, so `Paren<(), (), LangA>` satisfied `Delimiter<'_, L, LangB>` and a
    production copy-pasted out of a sibling dialect type-checked silently. The two are now the
    **same** parameter.

    As with the separators, the widening was never what made the fluent family work. Every
    `delimited_by_*` and `try_delimited_by_*` — on `ParseInput` and on all eleven many-builder
    surfaces — now instantiates the pair at the caller's language, so their return types gain
    that argument. The seven option builders (`AtLeast`, `AtMost`, `Bounded`, and the four
    leading/trailing options) are generic only over the parser they wrap and have no language
    to name, so their four `delimited_by_*` methods take the brand as a **method** type
    parameter instead; the driving impl's `Delim: Delimiter<'inp, L, Lang>` obligation unifies
    it with the context language. A builder stored in a `let` and never driven has to name the
    brand itself.

    The pair-typed error vocabulary moves with it, or two routes to one shape would disagree on
    the error type. `parens`, `braces`, `brackets`, `angles` and their attempt twins type the
    `Unclosed` they emit on the pair at their own language, and the twelve
    `Unclosed*`/`Unopened*`/`Undelimited*` aliases plus their twelve `Lang`-generic constructor
    blocks name it the same way — `UnclosedParen<S, Lang>` is now
    `Unclosed<Paren<(), (), Lang>, S, Lang>`.

    **Migration:** at `Lang = ()` every spelling is unchanged, since `Paren` already means
    `Paren<(), (), ()>`. A branded grammar spells the pair's brand in two places: a
    `TypedDelimiter` bound forwarded through a **generic** lexer (`Bracket<(), (), Lang>:
    TypedDelimiter<'inp, L, Lang>`), and a turbofish that names the pair
    (`delimited::<Bracket<(), (), MyLang>, …>`). At a concrete lexer the impl resolves and
    neither is written. The fluent methods need nothing. The fence is pinned by two
    `compile_fail,E0277` doctests on `Delimiter`, paired with a positive control differing from
    each in one token.

    — *(W-api-A, #122)*

*Choice dispatch.* An id type whose whole purpose is to make an out-of-range branch
unrepresentable was admitting one.

35. **Array-choice branch ids are 1-based: `<[P; N] as ParseChoice>::Id` is
    `RangedUsize<1, N>`.** `RangedUsize`'s bounds are *inclusive*, so `RangedUsize<0, N>`
    admitted `N` — one past the last index — and dispatching it panicked. `[P; 0]` was worse:
    `RangedUsize<0, 0>` admits `0`, so **every** id for an empty array choice panicked.

    The repair is a bijection rather than a bounds check: exactly `N` representable values,
    `1..=N`, onto the indices `0..N`, so every id that exists is a valid index. A bounds check
    would have left the boundary value constructible and the panic reachable, which is the
    thing the ranged id was for. The audit's own suggestion — `RangedUsize<0, {N - 1}>` — is
    not writable: a generic `N` in a const operation is `error: generic parameters may not be
    used in const operations`, and `generic_const_exprs` is unstable on every supported
    toolchain.

    `[P; 0]` moves from "every dispatch panics at runtime" to "no id exists, and constructing
    one is a compile error" (`E0080`, from `deranged`'s own range assertion, at the point of
    use — an unused `[P; 0]` choice still compiles).

    **Migration:** `Id::new(i)` for the old 0-based `i` becomes `Id::new(i + 1)`, and
    `Id::get()` returns the 1-based ordinal. The boundary value that used to panic no longer
    constructs.

    The two slice impls (`[P]`, `&mut [P]`) have no compile-time repair — length is a runtime
    fact — so they gain a **named** refusal instead: `choice id {id} out of bounds for {len}
    branches` where the raw `index out of bounds` message used to be, plus the documented
    bounds the audit assumed were already there.

    — *(R10)*

*Span mutators.* `SimpleSpan` enforced its ordering invariant on one of its four mutator
surfaces. The other three assigned and added without checking, so a corrupt span was produced
silently and carried onward — and the `## Panics` sections documented panics the code did not
perform.

36. **Every `SimpleSpan` mutator the crate owns now refuses to publish a corrupt span, in every
    build.** Six `*_const` twins gained the ordering assert their non-const twins already
    carried, with the twin's byte-exact message; `bump_start_const`, `bump_end_const`,
    `bump_const` and `<Range<usize> as Span>::bump` gained an every-profile `checked_add` with
    the message `"span bump overflows usize"`; and `<SimpleSpan<O> as Span>::bump` — the
    surface this crate's own error and carrier types relocate through — gained the ordering
    assert after its delegation.

    **This turns silently-corrupt results into panics on code that runs today.** Unlike a
    refusal that rejects a correct program, these surface a defect the program is already
    carrying: `with_end_const(2)` on `(5, 15)` produced `(5, 2)`, and `bump_start_const` at
    `(MAX - 1, MAX)` produced `(0, usize::MAX)` in release with the one existing assert passing
    over the corruption. The migration line is *"your span was already inverted — here is
    where"*.

    **Two surfaces deliberately do not get an ordering assert, and the reason is a law rather
    than an omission.** `bump` shifts both ends equally, so on a well-formed span it can only
    fail under wrap; on an ill-formed one it fires on a precondition the caller did not
    violate. So the ordering assert is licensed by an impl's **construction-axis** strictness.
    `SimpleSpan` refuses inverted construction, so its assert can only catch a wrap — it stays.
    `Range<usize>` documents *lenient* construction (`Span::new(10, 5)` yields `10..5`), so an
    inverted range is a value the trait says you may create; asserting on relocation would hand
    out a value you are then not allowed to use. It keeps the overflow check only.

    The inherent `SimpleSpan::bump` also stays unguarded, and that is priced rather than
    overlooked: guarding it means adding `O: Ord`, which is a real narrowing with a reachable
    inhabitant — `SimpleSpan` derives `Default`, so `SimpleSpan::<NonOrd>::default().bump()`
    compiles and runs today. The guard sits on the `Span` impl instead, whose where-clause
    already carries `Ord`, so it costs nothing. The residue is an in-crate caller that bypasses
    the trait.

    **Stated residue, and it reaches the public non-const mutators too.** On a generic `O`,
    overflow is rustc's own check — a panic in debug, a wrap in release. `SimpleSpan::bump`,
    `bump_start` and `bump_end` are all generic, so `checked_add` is not expressible there
    (`E0599` on a type parameter), and in release the ordering assert catches a wrap **only
    when the wrapped value lands on the wrong side**. `bump_start` at `(MAX - 1, MAX)` wraps
    the start to `1`, still `<= end`, and publishes silently; `bump_end` on `(0, MAX)` wraps
    the end to `2`, still `>= start`, likewise. Both are pinned by release-profile cells, so
    the gap is visible rather than implied.

    **The checked surface for `usize` already exists, and both methods' docs now name it:** the
    `*_const` twins perform the same operation on the concrete type with a real `checked_add`,
    and a `const fn` is callable at run time. Their `## Panics` sections previously promised a
    panic the release build does not perform — the exact fiction this item exists to remove,
    which survived on these two until it was pointed out.

    Closing it *in the generic methods* needs an arithmetic bound on `Span::Offset`, which core
    cannot express — a trait definition or a dependency, plus a breaking narrowing. That stays
    a 0.9 candidate.

    The `Span` trait itself gains the written law (the two axes above) rather than a check:
    `bump` is required, so an implementor's body is theirs. `start_mut` / `end_mut` remain open
    doors by construction — an inversion written through them is unobservable to any assert on
    any mutator.

    — *(R10)*

*Rendered output.* One composition rule, applied at every carrier that violates it.

37. **Error renders say "expected" once.** `Expected`'s own `Display` opens with the word in
    every variant (`expected '}'`, `expected one of: …`), and two carriers wrote it again when
    composing one — so `MissingToken` rendered `missing token 'comma' at 5, expected expected
    '}'` and `UnexpectedToken` rendered `unexpected token, expected expected '}'`. The
    composing sites now supply only the separator.

    **`UnexpectedToken` was the unpinned one, and that is why its stutter outlived the
    repair.** Four pins recorded `MissingToken`'s doubled output as long-standing behaviour;
    `UnexpectedToken`'s renders were pinned nowhere, at any of its three shapes. Both carriers
    are fixed in one commit, and all shapes of both are now pinned byte-exactly.

    **Migration:** a consumer that freezes these renders re-blesses; the delta is exactly one
    deleted word per composed render. A consumer that converts the carriers structurally
    through `From` — which is what the reference consumer does — sees nothing.

    — *(R10)*

*Typed CST views.*

38. **`cst::Node<Lang>` binds `Syntax<Lang = Lang>`.** The supertrait list was
    `Element<Lang> + Syntax`, leaving `Syntax::Lang` free — so a type could be `Element<A> +
    Syntax<Lang = B>` and still satisfy `Node<A>`: a node claiming membership in one language
    while the `KIND` constant that says which node it *is* answers for another. Nothing rejected
    it, and the contradiction surfaced later as a cast that never matches.

    **Implementor-breaking:** a downstream `Node` impl whose brands disagree stops compiling.
    That is the fix. Every in-tree impl and the reference consumer brand coherently already.

    — *(R10)*

*Test-support surface.* Last, because it is opt-in: the `fuzz` feature's alphabet is public
API and moved like any other.

39. **`fuzz::Op` gains an `IsExhausted` variant.** The fuzz alphabet tracks the real public
    input surface one-for-one, so `InputRef::is_exhausted()` earned an op. `Op` is an ordinary
    exhaustive enum: a downstream `match` over `Op::ALL` without a wildcard arm stops
    compiling, which is deliberate — the same compile-time exhaustiveness prod the crate's own
    `Op::label` relies on. `Op::COUNT` moves with it, and the variant was inserted in surface
    order rather than appended, so the fieldless discriminants of every later variant shift by
    one. Nothing in the crate casts them and `Op` carries no `#[repr]`, so the values were
    never a contract — recorded because a mechanical API diff reports them and a reader should
    not have to wonder whether it matters.

    *This item exists because a `cargo semver-checks` run against the published 0.7.3 found
    it, and no PR body, commit message or surface grep had.*

    — *(R5, #116)*

*Diagnostic counts on the recovering path.* Last in this list because it removes a report
rather than adding one, and only where that report was a duplicate.

40. **A wrong opening delimiter is reported once, not twice.** All four delimited many-drivers
    (`repeated`, `repeated_while`, `separated`, `separated_while`) probe the opener, and on a
    wrong token they record it as an expected-open `UnexpectedToken` and leave it **cached and
    unconsumed**. The close-position probe later re-saw that same cached token and reported it
    a second time, as an expected-close miss. Under a recording emitter both landed; the two
    entries carried an identical span.

    The report is now suppressed **iff the emitter's log still holds a live report naming that
    front token** — and the witness for that is a **lineage fact on the input**, not a flag in the
    driver's stack frame.

    That distinction is the substance of the fix. "Has this token already been reported?" is a
    *transactional* question, and a parser's stack frame has no rollback semantics. A driver-local
    flag — committed position, with or without a re-key counter beside it — cannot observe a
    restore, because a restore is exactly the operation that erases frame-visible traces: it
    rewinds the emitter, destroys the stream's front, and returns position to a value the frame
    cannot tell apart from "nothing happened". Three successive frame-local witnesses were built
    and each leaked, always toward the same failure: a **deleted** diagnostic.

    So `Input` now carries a front-report watermark. `Some(end)` means the emitter's current log
    holds an unexpected-token report whose subject is the token the current regime lexes at the
    stream front, ending at `end`. Three writers keep that inductive, and no others: it is
    published by the same body that appends the report and only if the append succeeded; it is
    cleared by a re-key, before the cache clear, so an unwind through that caller code leaves it
    disarmed and a later arm emits; and it is captured into every `Checkpoint` and restored in the
    same no-caller-code block as the emitter rewind. A rollback below the flag truncates the report
    and disarms the watermark as one state; a rollback above it keeps both, because the report
    predates the mark.

    The general law, now stated in the cell census: **a witness must share the rollback semantics
    of the fact it witnesses.** The cache-push counter is restored because a restore drops exactly
    the entries it counts; this watermark is restored because its subject is the emission log, and
    the log is restored in the same move.

    A close miss naming a **different** token, or arriving once the report for the flagged one is
    no longer live, is untouched: `"{1 2 x"`-shaped input keeps both of its reports.

    Because the predicate reads input state rather than a driver local, all eight close-miss arms
    ask it with one identical line — so uniformity across the drivers is a property of the
    mechanism rather than a claim about each driver's control flow.

    **Only recording emitters are affected.** Under a fail-fast emitter the first emit aborts
    the driver, so the second report never existed there and no `Fatal`-path behaviour moves.
    What changes is any assertion that counted diagnostics on the recovering path.

    The suppression is applied at **all eight** `CloseStatus::WrongToken` arms across the four
    drivers, not only the four that today's tests reach — gating a subset is how a fixed defect
    relocates to its own sibling.

    — *(R11, D43(b))*

*The Pratt engines' recursion budget, and the non-associative contract.* Appended after item 40
rather than renumbered into the Pratt group at 21–26, because every number in this section is a
live cross-reference. Inside the group the breaks that do not fail to compile come first, as
everywhere else.

41. **A non-associative operator that repeats is a syntax error, where it used to fold left in
    silence.** Once a frame has folded a `PrattInfix::Neither` operator at
    power `p`, a second *infix* operator at exactly `p` — whatever its own associativity — now
    fails the parse with `NonAssociativeChain`, with the offending operator left on the input
    unconsumed. Both engines raise it, on the same trigger, with the same restored posture, and it
    is **returned** rather than emitted, so no recording emitter and no rewind can absorb it.

    Previously the repeat was simply admitted. At the top level that merely left a tail behind —
    `1 ; 2 ; 3` folded to `(1;2)` — but nested, it had no caller-side remedy at all:
    `7 = 1 ; 2 ; 3` returned `Ok(((7=(1;2));3))` with the **whole input consumed**, so nothing was
    left over for an end-of-input check to catch. That shape is the reason this is an error and not
    an ending. Non-associativity is a property of a *chain*, and the only frame that knows the
    chain exists is the one holding the latch; hand the operator up instead and the recipient is
    the same engine one frame up, which sees an ordinary admissible operator, folds it by its own
    rules, and re-associates the chain across itself. It is structurally incapable of knowing the
    constraint.

    **The contract is per-operator, not whole-chain fixity resolution.** The latch is cleared by
    folding an infix at a different power and is untouched by a postfix fold, so `a == b < c` —
    this variant, then `Left` at the same power — is rejected while `a < b == c` is accepted.

    **Not terminal**, deliberately: this is malformed input, the classic recovery target, so
    `Recover`, `InplaceRecover` and `skip_then_retry` may spend it. A grammar that wants the old
    fold-once-then-stop behaviour asks for it in grammar code — wrap the pratt parser in a recovery
    combinator, or reclassify the operator `Left` — because tolerance is a caller policy and not a
    silent engine default. Where a repeat coincides with a zero-consumption report, item 25's stall
    outranks it: the report boundary is checked first, since a driver must not diagnose a chain
    built out of a contract violation.

    — *(R12; the two exits ranked in R13)*

42. **Recursive descent is bounded, and the bound is on by default.** Both Pratt engines enter one
    level per live frame through the new `InputRef::descend`, whose `Descent` guard releases the
    level on **every** exit of the frame — return, `?`, or unwind, identically in `std` and
    `no_std` — and exceeding the budget fails the parse with the always-terminal
    `RecursionLimitReached`. It too is returned rather than emitted, so a tripped budget cannot
    reach a caller as a truncated-but-successful parse.

    Previously the descent was bounded only by the token count, which is not a bound a *machine*
    has. The red-before probe for this item did not fail an assertion; it **aborted the test
    process** with `fatal runtime error: stack overflow`.

    The budget is per *input session* rather than per parser — two expression parsers composed into
    one grammar draw on one depth, which is what makes it a bound on native stack use rather than
    on any single production — and it is configured with the new
    `InputContext::with_recursion_limiter` / `ParserContext::with_recursion_limiter`.
    **`RecursionLimiter::unlimited()` restores 0.7.3's behaviour exactly**, at 0.7.3's risk.

    **The default is 64, and it is sized against the tightest measured configuration rather than
    the most generous one.** Bisected on this tree, one pratt frame per level, on an explicitly
    sized **2 MiB** thread — what `std::thread::spawn` and every libtest harness thread gets, and
    so the smallest stack a parse is likely to run on:

    | build | typed driver | token driver |
    |---|---|---|
    | release (`opt-level = 3`) | 3871 frames | 4247 frames |
    | debug (`opt-level = 0`) | 384 frames | **125 frames** |

    The binding cell is the debug token driver at 125, not the release figures above it, and the
    asymmetry is the whole argument: a limit that is too low returns a clean, catchable error
    naming the knob that raises it, while one that is too high aborts the process with no
    diagnostic and takes the suite with it. Only one of those is recoverable, so the default is set
    where every measured configuration survives. 64 is the largest power of two leaving ~1.9×
    margin under 125, and it clears the other three cells by 6×, 60× and 66×. A grammar parsing
    untrusted, deeply nested input should still set its own limit against the stack the parse will
    actually run on, rather than inherit this one.

    **Your own recursive combinators draw on the same budget, and `InputRef::descending` is how.**
    It takes the frame's body as a closure and owns the level for exactly that body:

    ```rust,ignore
    inp.descending(|inp| match remaining {
      0 => Ok(inp.recursion().depth()),
      n => nested(inp, n - 1),
    })
    ```

    The closure's error is returned untouched, so `?` inside composes with whatever the frame
    already returns and the trip is built as the frame's own error type; a body written entirely
    as the closure keeps its `return`s; a panic releases the level on the unwind. Nothing the
    closure can write releases it earlier — it is handed the input, never the guard, and
    `InputRef::recursion` is read-only.

    **`InputRef::descend` is also public, as the low-level escape hatch, and it is easy to write
    wrong.** It hands the level back as an ordinary value, so *where the level ends is caller
    code*. `Descent` is `#[must_use]`, which catches **one** spelling of getting that wrong and
    not the others. Measured on this tree at limitation 8, over 200 recursive calls, with the
    abort depth bisected one process per depth on a 2 MiB thread:

    | frame body | diagnostic | at 200 calls | aborts by |
    |---|---|---|---|
    | `inp.descending(\|inp\| …)` | — | `Err(RecursionLimitReached { depth: 9, limitation: 8 })` | never |
    | `let mut frame = inp.descend()?; let inp = &mut *frame;` | — | same | never |
    | `inp.descend()?;` | `unused_must_use` | `Ok`, depth 0 | 5 000 |
    | `let _ = inp.descend()?;` | none | `Ok`, depth 0 | 5 000 |
    | `if inp.descend().is_ok() { … }` | none | `Ok`, depth 0 | 5 000 |
    | `let d = inp.descend()?.recursion().depth();` | none | `Ok`, depth 1 | 4 000 |
    | `drop(inp.descend()?);` | none | `Ok`, depth 0 | 5 000 |

    An abort is `fatal runtime error: stack overflow` — the failure this whole item exists to
    delete, reinstated by a line that compiles. The abort depths are a demonstration rather than
    a constant: they are one debug build's frame size, probed at 1 000-level steps, and one row
    moved a whole step between two builds of the same source. The four silent rows are **not a
    closed list**:
    any expression that consumes the guard and lets it die before the recursion behaves the same
    way, and rustc's own `help:` on the caught row suggests the second one. Early release cannot
    be forbidden, because it is sometimes what a frame wants, so what closes the question is the
    *shape*: `descending` makes the level's scope and the body the same region by construction,
    the bound guard makes them the same region by discipline. What the type system guarantees on
    its own — for both spellings — is *balance* (every level entered is left exactly once, by the
    destructor, unwind included) and that a **live** guard is the only route to the input.

    Rails: `tests/ui/descent_dropped_early.rs` pins the attribute on the MSRV toolchain,
    `src/input/input_ref/descent_tests.rs` pins every row of the table above at runtime on every
    toolchain, and its `#[expect(unused_must_use, …)]` turns a deleted attribute into a hard error
    under the crate's `deny(warnings)`.

    — *(R12; the default resized against measurement in R13; the guard marked `#[must_use]` in
    R22; `descending` added and the other three early-release shapes measured in R23)*

43. **`RecursionLimiter`'s default depth was dropped from 500 to 64 in every constructor that
    supplies one, then reverted before release.** `RecursionLimiter::new` and its `Default`, and
    `Limiter::new` / `Limiter::with_token_tracker` — which build one for you — carried the 64
    default only during the campaign. [50](#0.8.0-changed-breaking) returned all four to 500
    before 0.8.0 shipped: what 0.8.0 ships is 500 for every one of them, and only `ParserContext`
    and the input layer keep 64 — neither routes through these constructors. This entry is kept
    because [50](#0.8.0-changed-breaking) and the migration table's row for these four
    constructors both rely on the reasoning below — read the two together, and act on 50.

    **The reasoning:** these four constructors reach code with no pratt parser in it. The same
    type doubles as a **lexer-side** nesting tracker in a `State` / `Extras` position, where it
    counts lexing nesting, costs no native stack, and where 500 was never sized against anything.
    Had the drop shipped, a lexer that inherited the default and nested deeper than 64 would have
    tripped where it had not before.

    Spell the limit you meant with `RecursionLimiter::with_limitation`. The one `500` this round
    left standing in the docs was the lexer-extras example in `Limiter`'s own documentation, kept
    deliberately: there the number is the example's explicit choice rather than a default, and the
    two subjects are worth keeping apart.

    — *(R13)*

44. **`NonAssociativeChain`'s offset is *defined* as the handback position** — the driver's own
    restore target — and not a position measured anywhere near the offending operator. `1 ; 2 ; 3`
    reports **5**, where a formulation written earlier in this campaign reported 6.

    The definition is checkable rather than descriptive, and that is the point of it: catch the
    error and **`InputRef::span().end()` *is* the offset**. Everything from that byte onward is
    still available to the caller — including whatever the deciding read had consumed before it was
    handed back.

    That identity is about the **handback**, and it is not a statement about where a recovery
    combinator restarts. `Recover` and `skip_then_retry` speculate through `try_attempt`, whose
    failure path restores the pre-attempt checkpoint before the handler runs or the skip loop
    starts, so both resume at their own attempt origin instead; `InplaceRecover` never backtracks
    and is the one that stands where the offset names. Measured on `1 ; 2 ; 3` with the whole pratt
    parser wrapped, where the error carries 5: catching the `Err` yourself reads 5,
    `inplace_recover` reads 5, `recover` reads **0**, and `skip_then_retry` scans forward from
    **0** — its first sync candidate is the `1` at 0 and its first skipped region is `0..1`.

    Reading it as a pointer at the operator is wrong in four ordinary shapes, each measured:
    whitespace the lexer skips (`1 ; 2 ; 3` — offset 5, the repeat at 6); whitespace surfaced as
    trivia *tokens* and skipped by the classifier (offset 5, the repeat at 8); an operator spelled
    with two tokens (offset 7, head at 8, tail at 10); and a non-fatal lexer error, reported and
    stepped over (offset 5, the repeat at 8). Naming the operator would describe a position no
    caller was ever left at, and would invite a recoverer resuming there to discard the region in
    between — in the last shape, the lexer error's own bytes together with the diagnostic that was
    rewound with the aborted probe and is due to be re-emitted. The operator's head is not
    obtainable in general anyway: `parse_pratt_rhs` holds a whole `InputRef` and decides for itself
    where its operator begins, so the only way to learn what it would skip is to run it — and the
    repeat has to be decided before it may.

    **`Display` names it the same way**, because the text is what a caller who never calls
    `offset()` still has: `non-associative operator cannot be chained at its own power; input
    handed back at 5, at or before the operator`. It does not report the operator's position,
    for the reason above — the engine does not have it. See *Debug and rendered output*.

    — *(R14 defined it, R15 made the number the restore target)*

45. **`InputContext::into_components` returns `(E, C, RecursionLimiter)`** where it returned
    `(E, C)`. The context carries the recursion budget now, and a decomposition that dropped it
    would hand the input an unconfigured one. `InputContext::new`'s arity is unchanged — the budget
    defaults, and `with_recursion_limiter` sets it — so only the destructuring moves, and `..` is
    not available on a tuple pattern, so the compiler points at every site.

    — *(R12)*

46. **Both Pratt engines require two more conversions from a grammar's error type.** The token
    engine's `pratt`, `pratt_with_min_precedence` and `pratt_in`, and the typed driver's
    `ParseInput::parse_input` impl block, gain `From<RecursionLimitReached<L::Offset, Lang>>` and
    `From<NonAssociativeChain<L::Offset, Lang>>`. Two `From` impls on your error type; every
    example and bench in this repo carries the shape.

    **`From`, and not a `FromPrattError` method, because these two never pass through an
    emitter.** Neither has an `emit_*` counterpart on `PrattEmitter`, by design — a recording
    emitter must not be able to swallow a resource trip or a rejected chain — so the engines
    *return* them, and a returned error becomes the caller's type through `From`. The trip's
    value is built and converted at `InputRef::descending` / `InputRef::descend`, the recursion
    entry every recursive-descent grammar shares whether or not it has a pratt parser in it; the
    repeat is built by each engine at its own exit. Neither point has an emitter in scope, so a
    conversion method on the emitter-side bundle would be one nothing could ever call.
    `FromPrattError`
    keeps exactly the two conversions a `PrattEmitter` body performs, and **no hand-written
    `FromPrattError` impl has to change.**

    The choice this obligation carries is unchanged, only its spelling: an impl that *stores* the
    trip keeps its payload — offset, depth, limitation — readable, and its always-terminal marker
    readable through `MaybeTerminal`; one that discards it loses both. What discarding no longer
    costs is the **stop**: item 48 moved the resource-trip witness onto the input session, where no
    `From` the grammar writes can reach it. Both are legitimate, and a `From` impl is the author's
    own code either way, so neither is picked for you.

    — *(R12, R21)*

47. **A gap is now tiled where it opens — in the node of the token it trails — not where it is
   noticed.** Where an uncovered run of source bytes lands no longer depends on what happens
   after the run is already determined. A run used to be tiled at the token that *revealed* it,
   or, for the **trailing** run, at the end of the walk; both are moments at which the parse may
   have left the node it was in when it stopped covering the source. So the same garbage produced
   two tree shapes, chosen by whether more input happened to follow: for a lossless dialect the
   tail landed *outside* the document node while identical garbage mid-document landed inside it.

   The rule now: an uncovered run opens the instant the token before it settles, and it is tiled
   there, in the node open at that moment. It is in the tree before the next event is read, so
   nothing that follows can move it. One clause covers the run that **no token precedes** — a
   source starting with bytes no token claims — which tiles where the walk first sees it: at the
   first committed token, or, if the parse committed no token at all, at the end of the walk in
   whatever node is open there.

   `Root[Document[Tok] Gap]` is now `Root[Document[Tok Gap]]`, and the node **widens over** the
   run it takes — a `Document@0..11` before a four-byte tail is `Document@0..15` after. A run
   nested deeper goes deeper: it lands at the depth of the token it trails, not one level below
   the root by fiat.

   Two placement laws hold by construction and are pinned over a corpus rather than as examples.
   *Appending a token never moves a gap*: two streams sharing a prefix through the token a run
   trails place that run in the same node, including when one of them stops there. *Hoisting a
   lexer error never moves a gap*: placement reads the token and structure events only. The
   second matters because a prefilled lookahead cache emits the lexer errors it crosses when it
   crosses them, so prefetching moves such a diagnostic earlier in the event stream — the token
   stream is exactly invariant under prefill and the diagnostic stream is not, and a rule reading
   a diagnostic's position would make the tree a function of how far the caller peeked.

   Unchanged: a **leading** run still tiles at the first token that follows it, inside whatever
   node that token lands in; a run trailing a root-level token stays a root child; a source with
   **nothing lexable in it** keeps its run beside the document node (`Root[Document@0..0,
   Gap@0..len]`), because there is no token for it to trail and the identical parse with one
   lexable byte appended puts it at the root too; and
   [`finish_partial`](https://docs.rs/tokora/latest/tokora/cst/struct.Sink.html#method.finish_partial)
   on an **unbalanced** stream still tiles a token-less run into the innermost *open* node. On a
   balanced stream both doors agree, as they did before.

   `tree.text() == source` is untouched: every uncovered byte still tiles, byte-exactly, under
   every one of these placements. Only the shape moved.

   **Re-bless any snapshot of a tree built over a source with an uncovered run that follows a
   committed token** — a truncated parse, an unterminated string or block string, a stray
   unlexable punctuator after real input. Consumers that walk by text see no change; consumers
   that walk by node, or that assert on a rendered tree, see the run inside the node of the token
   before it. A source with *no* committed token at all is not affected.

48. **A recursion-limit trip is now terminal for every grammar error type, `()` included — neither
   recovery nor a collection driver's failure path can spend it.** Two more of a collection
   driver's exits can reach a construct's end without the trip ever surfacing as an `Err`, and both
   close later in this entry: an element's absence exit (item 56) and a real closer committed just
   after one (item 57). A fourth exit does not close, and is not going to: an element that catches
   the trip and still answers `Accept` spends it, for every error type, on purpose. Decline lets
   the *driver* manufacture the construct's end from a stop the caller is never told about, which is
   why the driver has to guard its own conclusion; `Accept` means the *element* produced the value,
   and the driver is faithfully collecting what it was handed, not concluding anything of its own.
   tokora cannot stop a grammar from catching an error and returning a value without diagnosing
   it — true of every error a grammar can catch and answer, not only this one — so gating `Accept`
   would forbid a value-producing element from ever recovering from a budget it deliberately
   caught: a broader contract than #148 establishes. Item 56 states the exemption at the point it
   first applies; `parser::many`'s module docs carry it as the standing contract, not an
   afterthought.

   This closes 0.8.0's [known limitation — a discarding error sink erased
   the recursion trip's stop, not its
   bound](#0.8.0-known-limitation-recorded-and-closed-before-release--a-discarding-error-sink-erased-the-recursion-trips-stop-not-its-bound),
   which recorded the behaviour rather than answering it. The answer is that a resource bound an
   unrelated error sink can opt out of is not a bound.

   [`RecursionLimitReached`](https://docs.rs/tokora/latest/tokora/error/struct.RecursionLimitReached.html)
   has always been terminal for every value, but
   [`recover`](https://docs.rs/tokora/latest/tokora/trait.ParseInput.html#method.recover),
   [`inplace_recover`](https://docs.rs/tokora/latest/tokora/trait.ParseInput.html#method.inplace_recover)
   and
   [`skip_then_retry`](https://docs.rs/tokora/latest/tokora/trait.ParseInput.html#method.skip_then_retry)
   read that off the **converted** value — after the grammar's `From` had run. A `From` that
   discards the payload discards the marker with it, so a `()`-errored grammar got
   `is_terminal() == false` back and recovery **spent** the trip.

   **Where resource terminality is stored now:** on the **input session**, in a monotone counter
   (`Input::resource_trips`, crate-internal, bumped by
   [`InputRef::descend`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.descend)'s
   trip arm before the grammar's `From` runs, so a panicking conversion cannot skip it). The three
   combinators read it **beside** `MaybeTerminal::is_terminal`. No conversion the grammar writes
   can reach it, and nothing lowers it: a `Checkpoint` does not carry it and a restore does not
   touch it. Grammar error conversion therefore cannot affect it — which is the question 0.8.0's
   entry left open, answered here in the terms it asked for.

   **What the cell records and what the combinators test are different questions, deliberately.**
   The cell is a session fact — *this parse exceeded a budget, this many times* — and it is never
   cleared, because nothing can un-exceed a budget. Every site that consults it is judging **one
   attempt**, so it snapshots the counter before that attempt and re-raises only when the count
   moved *during* it. The two answers come apart exactly where grammar code catches a trip itself
   and parses on: the session has tripped forever after, while the next attempt is an ordinary one.
   Reading the session fact there would refuse recovery — and, below, emit-and-continue — for every
   later failure in the document, ordinary syntax errors included. A real trip is still re-raised
   wherever one happens, a second trip after a caught first one included.

   **What changes for you, and only if your error type discards the value:**

   | your grammar's error type | before | now |
   |---|---|---|
   | stores it and delegates `is_terminal` | re-raised | re-raised — **unchanged** |
   | `()`, or any `From` that drops it | recovery ran; `skip_then_retry` skipped and committed | re-raised |

   Concretely, measured on one ladder and one 32-deep input: `.recover(..)` used to return
   `Ok(<the recoverer's synthesized value>)` and now returns `Err`; `.skip_then_retry(..)` used to
   hand the surrounding grammar back offset **68** — the whole chain and the sync token consumed
   and committed — and now hands back **0**, which is the number a delegating error type always
   read. Same ladder, same limit, two error types, one answer.

   **If you relied on the old behaviour**, the fix is the budget, not the sink: raise it with
   [`with_recursion_limiter`](https://docs.rs/tokora/latest/tokora/struct.ParserContext.html#method.with_recursion_limiter).
   Recovering from an exhausted depth budget was never sound — no quantity of skipped input makes
   the next descent shallower, so those cycles spent input for a verdict they could not change. If
   you want the trip's *details* (offset, depth, limitation), that still requires an error type
   that stores the value; the discarding sink loses the payload exactly as before.

   **The same stop now also reaches the resilient collection loops — and that half was never
   sink-dependent.** `repeated`, `separated` and their delimited forms swallow an element's `Err`
   by design: emit it as a diagnostic and keep looping. Their gate re-raised the frontier
   `Incomplete` and the *scanner*'s committed boundary, and a descent trip latches neither — it has
   a control stack rather than a position, so there is no boundary for it to latch. A trip inside
   an element was therefore filed as an ordinary diagnostic and the loop went on to the next one,
   bounded only by the no-progress guard. Unlike the recovery half above, that happened for **every**
   error type: a delegating one was spent there exactly as `()` was. The gate reads the session
   counter too now — and, like the recovery half, it reads it against a baseline taken **once per
   element**, so what re-raises is a trip *that element* caused.

   | driving an element whose own attempt hands the trip back as `Err` | before | now |
   |---|---|---|
   | `repeated()`, `separated_by(..)`, and both delimited forms | the trip filed as a diagnostic; the loop continued to the next element and returned `Ok` with whatever it collected | the collection returns `Err` at the first trip, with **nothing filed** |
   | the same, for an element that fails **ordinarily** after some earlier construct's trip was caught and parsed past | the failure filed as a diagnostic; the loop continued | unchanged — the failure filed as a diagnostic; the loop continues |

   **This half changes behaviour for delegating and discarding error types alike**, so a grammar
   that used to receive a truncated-with-diagnostics container over a too-deep input now receives a
   failed parse. The remedy is the same one as above and for the same reason: raise the budget with
   `with_recursion_limiter`. A container assembled from elements the budget forbade reading was
   never a description of the input, and the diagnostic that named the trip was filed against a
   construct the parse had already been told to stop reading.

   **The scope of the change, stated rather than left to be found: it is the element's own trip
   that ends the collection, not the session's.** A parse that catches a trip and goes on keeps
   emit-and-continue recovery for the constructs after it — which is what an editor or language
   server needs, since one deeply-nested expression must not suppress the rest of the file's
   diagnostics. The same holds one nesting level up: an inner collection's trip that an element
   swallows is not charged to the enclosing collection's next ordinary failure.

   **The granularity floor, because "the constructs after it" has a resolution.** The witness is a
   counter, and a counter proves that *a* trip happened during the unit being judged — not that the
   error being judged *is* that trip. The unit is one attempt for `recover` and `inplace_recover`,
   one retry cycle for `skip_then_retry`, and one **element** for the collections. So grammar code
   that catches a trip itself and then fails **ordinarily inside that same unit** has the ordinary
   failure re-raised rather than recovered or filed. Move the catch one construct further out and
   it recovers, or is filed and looped past, exactly as an untripped parse would; that contrast is
   pinned by a test on each side of it.

   The floor **fails closed** at the recovery, failure, absence and real-closer gates: a real trip
   that reaches one of them is never recovered from and never filed as a diagnostic, and the only
   over-charged case is an ordinary failure that shares its unit with a caught trip. It says nothing
   about `Accept`, the fourth exit above: an element that catches a trip and still answers `Accept`
   spends it, for every error type, on purpose.

   The floor cannot be lowered by reading the error, because the error type is allowed to be `()` and
   discard the trip — which is why the witness is on the input in the first place. Lowering it
   needs a cooperative *rebaseline* published for code that deliberately catches a trip and wants
   the enclosing baseline moved past it. That is not in this release; no public name changes if it
   is ever added.

   — *(#148 R1)*

49. **An unpaired settle in the CST `Sink` now panics in release builds too, instead of silently
   shearing the two logs.** `<Sink as Emitter>::rewind` was already a wall against a *truncating
   rewind to a mid-log mark no live row captured* — a mark `checkpoint()` never returned, or one
   whose capture an earlier `rewind`/`release` already spent. That wall was
   `debug_assertions`-only. It is now unconditional.

   The reasoning it was built on had a hole. The debug-only posture deferred upward: the input
   layer's own LIFO witness was said to reject the condition on every input-mediated path, so the
   sink only needed a second wall for undisciplined raw use. But that witness is *itself*
   `debug_assertions`-gated, so a release build had no wall on **either** layer. What a release
   build did instead was keep the sink's own channels exact and leave the inner emitter untouched
   — the event log and the diagnostic log silently out of step, with nothing recording that they
   were. That is bounded only for as long as materialization reads the whole log in one pass at
   the end; anything that hands part of the tree onward before the parse finishes turns it into a
   wrong tree that no consumer can detect.

   The condition is a **parser bug and cannot be provoked by input**: a malformed document
   changes which branches run, not whether a branch settles its own capture exactly once. So
   hardening it cannot turn a bad document into a crash — verified across the malformed and
   truncated-prefix corpora, which produce the recovery holes and rewinds this touches and whose
   tree hashes are unchanged.

   **`release` is deliberately *not* hardened with it**, and that is the one asymmetry to be aware
   of. Its two non-top outcomes are specified behaviour rather than violations, mirroring
   `InputRef::commit`'s documented cost model: removing a row that is not the innermost is the
   "linear removal" that method already promises when a younger capture is still live, and finding
   no row at all is its "harmless no-op … (no panic, in any build)". Settles are newest-first only
   *within* a family — guards and session points interleave by design — so an out-of-stack-order
   release is lawful. Making either loud would convert a documented guarantee into a crash.

   **The panic is raised before the rewind's first mutation.** A wall placed after the damage
   only narrates it. The condition is decided by a read-only preflight over the unchanged mark
   stack, ahead of the row spend, the `events.truncate`, the undo journal's reverse replay and
   the era ledger's truncation record — every one of which the violating call used to have
   already performed by the time the check fired. A host that `catch_unwind`s the panic therefore
   keeps the sink it had, on every channel, instead of one whose event log was rewound and whose
   inner emitter was not; `finish_partial` on such a sink used to return that sheared state as a
   perfectly ordinary tree, with a `gap_kind` tile standing where the dropped token had been.

   **A panic already unwinding is exempt, and this is load-bearing rather than a hedge.**
   `Emitter::rewind` can run from a rolling-back guard's `Drop`, and a panic raised there is a
   *double* panic: the process aborts outright — no unwinding, no `catch_unwind`, signal 6. That
   is strictly worse than the shear being reported. The report is therefore suppressed when
   `std::thread::panicking()` is true — but suppressing the *report* is not licence to degrade
   invisibly, and this is where the mid-unwind path changed as well. It no longer leaves the sink
   half-rewound. It degrades to a **total no-op on every channel** (the sink has no correct
   rewind to perform, so it performs none, and both logs are left describing the same history)
   and **latches** the fact. `Emitter::rewind`'s contract is amended to state the general rule:
   what an implementation must never do is **abort**, so an emitter that can *detect* an unpaired
   settle may report it by panicking, provided it checks `std::thread::panicking()` first, raises
   before it mutates, and records any report it had to suppress.

   **New: `FinishError::UnpairedSettle { mark, len }`.** The latch is what a caller sees. After a
   degraded rewind, **both** materialization doors refuse — `finish` and `finish_partial` alike,
   because a log describing a rollback that never happened is corruption, not the incompleteness
   `finish_partial` exists to tolerate. A typed error rather than a panic, following the posture
   the neighbouring emission-time walls already take (a detect-at-cause assert, with a typed
   `FinishError` as the backstop at materialization) and preserving `finish`'s documented
   never-panics guarantee. `FinishError` is `#[non_exhaustive]`, so the new variant is additive;
   a `match` that does not name it keeps compiling.

   **What to expect if you are affected.** A parser that was quietly relying on the release-build
   degradation now panics at the cause with `Sink rewind to a mid-log mark with no captured row`.
   The fix is always at the call site, not here: some capture is being settled twice, or a mark is
   being rewound to after it was released. Debug builds and `cargo test` already panicked on it,
   so a parser whose backtracking paths are exercised in tests will see no change. Note also that
   `InputRef::restore`'s release-build promise is narrowed to match: it still makes no panic of its
   own after an out-of-order raw restore, but the *emitter* it hands the violation to keeps its own
   posture, and the `Sink`'s is now loud in every build. One consequence is documented rather than
   removed: a raw restore the sink refuses is **not itself transactional**. The emitter's state is
   untouched, but `restore` raises from the middle of its own rollback, so the checkpoint lineage
   has been popped through the target while the position and the reporting witnesses have not been
   restored. That is inside the "unspecified but bounded" envelope the method already documents,
   it is reachable only through the double-settle bug being reported, and it is now pinned by a
   test rather than assumed away.

50. **`RecursionLimiter::new` and the combined `Limiter`'s defaults return to 500 too — only
    `ParserContext` and the input layer keep 64.** The 500 → 64 drop above landed on one constant
    shared by more subjects than it should have. `RecursionLimiter::new` (and `Default`) is the
    tracker's own general-purpose depth, with no assumption about what a level costs.
    `ParserContext` and the input layer are the only two places a level IS a live native-stack
    frame: each holds its own `RecursionLimiter`, sized against a measured debug-build ceiling,
    for the budget a Pratt-driven parse actually descends against. `Limiter::new` and
    `Limiter::with_token_tracker` requested that same 64 by construction, but fed neither one —
    `Limiter` is not part of tokora's own parser wiring anywhere in this crate. Like bare
    `RecursionLimiter`, it is documented as usable directly as a lexer's `State`/`Extras` nesting
    tracker, where a level costs no native stack at all, and both of those lexer-facing paths
    inherited 64 anyway, tripping on a number chosen for a reason that does not apply to either.

    `RecursionLimiter::new`/`Default`, `Limiter::new`/`Default`, and `Limiter::with_token_tracker`
    all give back 500, unconditionally. Only `ParserContext` and the input layer still request
    the native-stack-safe 64 explicitly, from the crate-private
    `RecursionLimiter::PARSE_DEFAULT_DEPTH` — neither one holds a `Limiter` or calls
    `RecursionLimiter::new`, so neither can silently fall back to the general-purpose default,
    and a Pratt-driven parse still gets exactly the protection it had. Everywhere else changes: a
    tracker built through `RecursionLimiter::new`, `Limiter::new`, `Limiter::with_token_tracker`,
    or `#[derive(Default)]` over either type, with no parser anywhere near it, trips at 500
    again, not 64. That includes the depth set by [43](#0.8.0-changed-breaking) above for
    `Limiter::new` and `Limiter::with_token_tracker` — the same lexer-side reasoning
    that moved bare `RecursionLimiter::new` applies to them equally, and was missed there the
    first time. Spell the limit you meant with `RecursionLimiter::with_limitation` if 500 is
    still wrong for your grammar or your lexer.

68. **A CST parse mints its own sink from the source it reads, and no callback anywhere in the
    crate is handed the emitter again.** `cst::parse_lossless` and `cst::parse_lossless_partial`
    are now the **only** doors onto a recording sink: each takes the source once and hands that
    one argument to both the `Sink` it mints and the `Input` it drives, so the two names a
    caller used to be able to state independently — one at `Sink::new`, one at the parse entry —
    are the same argument of the same call. `Sink::new` is `pub(crate)` now; the type keeps its
    public name only as the thing a spent handle wraps. What a driver hands back is not a `Sink`
    but a `Cst<'inp, L, E>` — `finish`, `finish_partial` and the `with_trivia_policy` builder
    move here from `Sink` — and `Cst` **deliberately implements no emitter trait**, so the
    artefact a driver returns cannot be fed back in as a second parse's context. Minting closes
    the construction half of the wrong-source class; the handle's missing `Emitter` impl closes
    the return half.

    **A finish-time wall against the construction half was built during this campaign and
    removed before release.** It caught a naive wrapper, and each of three further shapes — an
    all-diagnostic parse, pre-arming through the public `bound_source` query, and a hand-emitted
    `cst_token` span — was then found to walk around it: every fix relocated the hole rather
    than closing it, *because the witness was flags on the sink and the sink cannot tell who set
    them*. See the known limitation below (revised in place for this item) for the full history
    and for what minting still does not reach.

    **The emitter never reaches a callback either, and that closes the same class from the other
    side.** `&mut Ctx::Emitter` **is** an emitter: code holding one could wrap it in a type of
    its own and install that wrapper as a second parse's context over a different buffer, which
    re-opens from inside exactly the hole minting just closed at the entry. Every public callback
    that used to take `&mut Ctx::Emitter` now takes an `EmitterView<'a, 'inp, L, E, Lang>`
    instead: `Decision::decide` (the condition of every `*_while` combinator), `ParseInput::peek_then`'s
    handler, `ParseChoice::peek_then_choice` and `peek_then_try_choice`'s handler, and the three
    token-level pratt folds, `PrattFoldTokenPrefix`/`Infix`/`Postfix`. (`peek_then_head`'s own
    handler takes no emitter parameter at all, before or after, so it is unaffected.) A view
    implements no emitter trait, does not `Deref`, and hands back no `&mut E` — there is nothing
    at the end of a method call to put in an emitter slot, and the value itself cannot stand in
    one. Its 24 forwarded methods carry the emitter's own names, so a callback body written
    against `&mut E` keeps every statement it had; only the parameter's type changes. **Five**
    members stay off the surface — `Emitter::checkpoint`, `rewind` and `release` (the checkpoint
    lineage the *input* owns), `Emitter::commit_token` (the auto-emission chokepoint) and
    `Emitter::commit_lexer_error` (its refusal-side twin, added by this item — see below) — for
    the same reason `InputRef` never gave a callback those either.

    **The handle closed its own version of the same door.** `InputRef::emitter` — the `&mut Ctx::Emitter`
    accessor — is `pub(crate)` now, and the new forwarding methods do not even route through it:
    each reaches the emitter cell directly, the same way `emitter()` itself always did.
    `ParseState`'s equivalent accessor is deleted outright, not narrowed — nothing in the crate
    called that one at all. Both gained the same forwarding surface `EmitterView`
    exposes, under the same method names, so `inp.cst_start(k)` does everything
    `inp.emitter().cst_start(k)` used to and there is nothing left over to put in a slot.
    `InputRef::emitter_ref` (shared) is public — **new here, not retained**: 0.7.3 and every build
    before this one had only the `&mut` accessor, and `Input::emitter` has always been
    crate-private, so reading a concrete emitter's own state from a grammar is a capability this
    item *adds* while removing the mutable one. It is safe to expose for the reason the mutable
    one was not: a `&E` cannot re-enter an emitter slot, because
    every recording method the trait family declares takes `&mut self`. Four getters that used to
    hand back `&mut Ctx::Emitter` outright now hand back an `EmitterView` in the same position
    instead — `peek_with_emitter`, `peek_with_emitter_terminal`,
    `sync_through_then_peek_with_emitter` and `sync_to_then_peek_with_emitter` keep their names
    and their callers' destructuring; only a call site that named the tuple's second type, or
    tried to re-wrap the reference, breaks.

    **One thing the closed door takes with it: `Marker::complete` is no longer callable from a
    parse.** It takes `&mut E` where `E: CstEmitter`, and after this item nothing public hands a
    grammar `&mut Ctx::Emitter` — `inp.emitter()` is `pub(crate)` (`error[E0624]`) and
    `EmitterView` is not a `CstEmitter` (`error[E0277]`), both reproduced against this branch. So
    `CompletedMarker` and `CompletedMarker::precede`, whose only producer is `complete`, are out
    of a grammar's reach with it. Nothing is removed or renamed: the verb still compiles for code
    that *owns* an `E: CstEmitter` (the shipped `Fatal` / `Silent` / `Verbose` / `Ignored` all
    implement it), and a grammar's route to the same two events is `inp.cst_start_at(mark, kind)`
    + `inp.cst_finish(kind)` off the new forwarding surface, which this release also adds. What
    is lost is the single-use typestate over that pair, since the raw route cannot consume the
    marker. `Marker` is not a leak in the other direction — it consumes a `&mut E` and never
    yields one — and the type's own docs now carry both shapes and this constraint. A
    `complete`-shaped verb over the view is a 0.9 question, not a late change here.

    **`CstEmitter::cst_token` is deleted, not deprecated.** It was the caller-chosen-span,
    no-consumption door — a grammar could pick which source bytes a token-shaped event names
    without any settle having happened — and nothing in the parse machinery ever called it.
    `Emitter::commit_token`, the input layer's own auto-emission chokepoint, was always the other
    producer of token events; with `cst_token` gone it is the **only** one.

    **The same door was open on the diagnostic channel, under another name, and this item closes
    that one too — `Emitter::commit_lexer_error` is new.** A recording sink's `finish` tiles a
    source byte no committed token covers *only* where a recorded lexer error covers it; an
    unexplained byte is a dropped committed token and is refused (`FinishError::UncoveredGap`).
    So a recorded lexer-error span is not a note, it is a **licence** — and until this change
    every route to `Emitter::emit_lexer_error` minted one, including the two that hand a
    caller-chosen span to a sink with nothing consumed for it: `InputRef::emit_lexer_error` (and
    `ParseState`'s re-export) and `EmitterView::emit_lexer_error`. A parser could excuse bytes it
    had simply walked away from, and through the orphan-rule wrapper described below a **foreign**
    parse's refusals — spans in a buffer this sink never saw — excused bytes of the original
    source. That is the `cst_token` shape exactly: a span the caller picks, licensing coverage,
    with nothing consumed.

    The fix splits the report from the evidence rather than removing the capability, because a
    parser must still be able to say *"this input is malformed here"* inline, with no rewind —
    the reason the `decide` family exists at all. `Emitter::commit_lexer_error` is the input
    layer's own door, called from its single deduped reporting site and withheld from both
    forwarding surfaces beside `commit_token`; the recording sink overrides it to record the
    coverage span. `Emitter::emit_lexer_error` keeps every caller it had and stays the diagnostic:
    the report reaches the inner emitter, occupies its slot in the rewindable log, and records no
    coverage span.

    **For almost every emitter this is invisible.** `commit_lexer_error`'s default body forwards
    to `emit_lexer_error`, so an emitter that only collects diagnostics — `Fatal`, `Verbose`,
    `Silent`, `Ignored`, and essentially every emitter downstream — implements nothing, receives
    every lexer error exactly as before, and cannot tell the doors apart. Two shapes must act.
    A **wrapper** emitter forwards `commit_lexer_error` the way it forwards `commit_token`;
    inheriting the default delivers the layer's refusals to the wrapped emitter as caller reports,
    and a wrapped recording sink then refuses a region that was legitimately unlexable. An emitter
    that *records* lexer-error spans as structural evidence overrides it. `LEXER_ERROR_CENSUS`
    (`src/input/input_ref/census_tests.rs`) and `COVERAGE_EVIDENCE_CENSUS`
    (`src/cst/sink/tests.rs`) lock the producer at one site and keep the method off the view.

    **What this does not close.** `EmitterView` implements no emitter trait *in this crate*,
    which is a fact about this crate, not a proof that no such implementation can exist: under
    the orphan rules a downstream crate whose own lexer type appears in the parameter list may
    write `impl Emitter for EmitterView<'_, '_, ItsLexer, Sink<…>>` and install that over a
    foreign buffer from inside a callback. It compiles, and it was run. What such a wrapper cannot
    reach is **either** producer of the sink's coverage machinery: `commit_token`, which is what
    pairs a span with a byte, and `commit_lexer_error`, which is what licenses a byte to have no
    token — neither is on the view's surface. Measured end to end: a foreign parse driven that way
    injects a node through the forwarded `cst_start`/`cst_finish` and reports its diagnostics into
    the log, while the sink still materializes its own text over its own coverage — no token, no
    span, no byte and no *licence* of the foreign buffer crosses. Stating that bound in terms of
    `commit_token` alone was incomplete through the release candidates, and the foreign-licence
    route it missed is now pinned from outside the crate by
    `an_orphan_view_wrapper_carries_a_foreign_lexer_error_but_licenses_no_gap`
    (`tests/parser_node.rs`), which returned a plausible round-tripping tree before the split and
    an `UncoveredGap` after it. The wrong-*text* class this item exists to close has no route
    through it; the node and diagnostic channels it reaches are ones a `decide` body could already
    reach directly, in its own parse, by design.

    **`Emitter::bound_source`, `Source::REFERENT_IS_BYTES` and `SourceIdentity` are retained, not
    retired**, and the known limitation below is corrected in place rather than left standing on
    a promise this item breaks — see it for the reasoning: the handshake is what a wrapper on the
    orphan route above has to forward honestly for the general constructor check to still catch a
    foreign pairing.

    **Migration.** A `decide`/`peek_then`/pratt-fold callback's emitter parameter changes type; a
    body that only calls methods by name does not otherwise change, because the replacement type
    is derivable from the function's own `Peeked<…>` parameter and `where` clause. Measured on
    the reference consumer: **43 one-line signature changes across 20 files, zero body changes,
    zero call-site changes.** A CST parse moves from `Sink::new` + `finish` to `parse_lossless` +
    the `Cst` handle's `finish`; a context pair now holds the sink **by value** rather than by
    `&mut`, so a `'sink` lifetime parameter disappears from any consumer type alias that named
    one.

    ```rust
    // before
    let sink = Sink::new(source, emitter, CstProfile::new(map_kind, ERROR, GAP));
    // (sink installed as the input's context, driven by the grammar — omitted)
    let (green, emitter) = sink.finish(ROOT);

    // after
    let (cst, parsed) = parse_lossless(source, state, emitter, profile, cache, grammar);
    let (green, emitter) = cst.finish(ROOT);
    ```

    **Performance: no regression.** Eight benchmark ids, three interleaved A/B rounds pooled
    against an archived baseline: a noise floor of 0.66–1.65% per id, and the largest excursion
    from it is **−0.912%** — inside its own floor, on every id. A regression below ~1% would not
    have been visible on this machine, so the honest reading is a null result, not a measured
    improvement. An attribution check backs it: a callback-cost regression from the new
    indirection would have moved the ids where the driver's own dispatch is ~30% of self time
    together and away from the ids where it is ~10%, and it did not — both groups moved with the
    noise, not against each other. Machine load 1.77–4.69 throughout.

    — *(#168)*

### Debug and rendered output

Every `Debug` / `Display` movement in this release, in one place, because a consumer who
freezes renders needs one list rather than a reading of the whole changelog. This subsection
exists because a private-field `Debug` delta broke a real consumer's frozen parity renders
during this campaign and cost a bisect — and because the *earliest* of these shipped four
rounds ago and was disclosed nowhere until now.

**Mechanism is stated per row, because it decides who has to review the next field
addition.** A *derived* `Debug` moves by itself the moment a field is added; a *hand-written*
one is a maintained surface that a field addition silently leaves stale unless someone edits
it. Both are listed, and which is which is not guesswork here — it was measured, and it is not
what an earlier inventory of this release assumed.

| Type | What moved | Mechanism | Round |
|---|---|---|---|
| `UnexpectedEnd` | gained a private `terminal: bool`, so `{:?}` renders it and `Eq` / `Hash` account for it — two errors differing only in terminal status stop comparing equal | **derived** | R1, #111 |
| `Unclosed` | gained the `kind` field | **derived** | #121 |
| `MissingToken` | gained the `name` channel; `Eq` / `Hash` derive over it | `Debug` is **hand-written** (`debug_fmt`), and already names the field | R4, #114 |
| `SeparatedError` | gained the `name` channel; `Eq` / `Hash` derive over it | `Debug` is **hand-written**, and already names the field | R4, #114 |
| `FinishError` | gained `InvalidDiagnosticSpan`, `MismatchedFinish`, `NonUtf8Source`, `InvalidDialectKind` | derived | R8, #123 |
| `CstProfile`, `KindValidator` | new types; `Debug` deliberately does **not** render the fn pointer (its derived form is a code address) | hand-written | R8, #123 |
| `PartialSession`, `Budget`, `SessionRefusal` | new types | `PartialSession` hand-written (pinned field list); the other two derived | R8, #123 |
| `MissingToken`, `UnexpectedToken` | `Display` drops the doubled word: `…, expected expected '}'` becomes `…, expected '}'` | rendering change, both carriers | R10 |
| `SeparatedError` | gained a `Display` it never had | new rendered surface | R10 |
| `RecursionLimitReached`, `NonAssociativeChain` | new types, so nothing frozen moves; listed because both are carriers a consumer will render — `Debug` derived, `Display` from `thiserror`. Each render names its offset the way the type's accessor does, not as a construct's location: `recursion limit reached at 15: depth 9, maximum 8` (committed consumption at the frame that could not be entered) and `non-associative operator cannot be chained at its own power; input handed back at 5, at or before the operator` (the handback of item 44 — on `1 ; 2 ; 3` the repeated operator starts at 6, and the render deliberately does not claim to name it). Both strings, and both derived `Debug`s, are pinned in `tests/render_freeze.rs` | derived | R12 |

**New panic messages.** A consumer that captures panic payloads — a test harness, a supervisor
loop, a `catch_unwind` host — sees new observable strings even though no `Debug` or `Display`
impl moved:

- `"end must be greater than or equal to start"` and `"start must be less than or equal to end"`
  on six `SimpleSpan` const mutators and on `<SimpleSpan<O> as Span>::bump`;
- `"span bump overflows usize"` on three const mutators and on `<Range<usize> as Span>::bump`;
- `"choice id {id} out of bounds for {len} branches"`, replacing the raw slice-index message;
- `"checkpoint id space exhausted"` / `"savepoint sequence exhausted"` /
  `"cache-push counter exhausted"` on the input's witness counters;
- the two source-identity refusals at the parse entry — a different source value, and a bound
  extent shorter than the parse reads.

**Of these, only the span messages are reachable from a program that runs today**, and that is
the point of them: they replace a silently corrupt span with a panic that names where it went
wrong. The rest fire only on misuse, on exhaustion of a 2^64 id space, or — for the
source-identity refusals — only where an unequal reference *proves* an unequal source, or where
the emitter is bound to a strictly shorter slice of that source than the parse reads. Neither is
something a correct program produces: the second refuses only the direction that can smuggle
structure, and leaves the longer-bound streaming direction alone.

<a id="0.8.0-added"></a>

### Added

New public items that arrived *as part of* a breaking change are described in full at the
item that carries them, and listed here only so scanning this section does not miss them:
`error::MaybeTerminal` (item 16), `ValueKeyedEmitter` (item 18), `SessionPointId` (item 19),
`MissingToken`/`SeparatedError`'s `with_name` and `name` (item 20), `PrattFloor` (item 23),
`DelimiterKind` and `Delimiter::KIND` (item 28), `SeparatorHandler::OBSERVES_SEPARATORS`
(item 5), `CstProfile` and `KindValidator` (item 13), `error::NonAssociativeChain` (item 41),
`error::RecursionLimitReached` with `input::Descent`, `InputRef::descending`,
`InputRef::descend`, `InputRef::recursion`, `RecursionLimiter::unlimited`,
`InputContext::with_recursion_limiter` and `ParserContext::with_recursion_limiter` (item 42),
`cst::parse_lossless`, `cst::parse_lossless_partial` and `cst::Cst` (item 68), `EmitterView`
(item 68). **`FromPrattError` is not on this list**:
item 46's two new obligations are `From` impls on your own error type, and the trait gains no
member, so a hand-written `FromPrattError` impl compiles unchanged.

- **The logos adapter works on 0.14 and 0.15, and is tested there.** `logos_0_14` /
  `logos_0_15` / `logos_0_16` were already separate features, but `tokora::logos` (and its
  `__private` twin) re-exported 0.16 only, and every one of the 79 integration test files was
  gated on the plain `logos` feature — which *implies* 0.16. The consequence was that the
  per-version CI legs compiled a crate whose adapter integration tests were all invisible to
  them: not one test they ran exercised a logos adapter.

  Both aliases now follow the same newest-wins precedence chain the adapter re-exports use
  (0.16 > 0.15 > 0.14), and the 118 `feature = "logos"` gates across 99 files became the
  three-version disjunction. `--features logos` still means exactly what it meant — the
  feature still enables 0.16 — so no existing consumer changes.

  Measured: the versioned legs now run the adapter integration tests at 0.14 and 0.15, all passing.
  Exactly one source construct was version-divergent across the whole surface — logos 0.16's
  `allow_greedy` nested attribute in one test fixture — and it is now a per-version
  `cfg_attr` split rather than a 0.16-only file.
  — *(R11)*

- **Oracle pins for the checkpoint capture window.** Taking a checkpoint (`save`, and so
  `begin`, `attempt` and session points) registers two things a later settle must find — a
  lineage entry and an emitter mark — and a `Checkpoint` has no `Drop` to settle them with, so
  an unwind between the first registration and the finished value would strand it where nothing
  could ever find it. The capture has run every fallible step ahead of both registrations since
  that was fixed, and nothing pinned it from the outside.

  Three tests now do, one per kind of caller code the window runs: a panicking `L::Span::clone`,
  a panicking `L::State::clone`, and a **foreign `Emitter::checkpoint` that panics**. Each
  asserts that the armed payload actually fired *and* that the world across the caught unwind is
  the world before it — live checkpoints, pins, emitter marks, cursor, span, recorded
  diagnostics. These are additive pins, not fixes: they were red before the reordering and are
  green now, and their red was demonstrated by mutating the capture order rather than claimed.
  The emitter cell is deliberately a law boundary — it asserts tokora's half only, because a
  mark the emitter never handed back is the emitter's to reconcile.
  — *(R11)*

- **Streaming sessions.** `PartialSession`, `Budget`, `SessionRefusal`, `RedriveFromBase` and
  the sealed `ReplayMode` give `parse_partial` a lifecycle: a partial parse survives its
  refills instead of restarting, under a budget that a caller sets and can observe. See the
  known limitation below before pairing a session with a recording `Sink`.
  — *(R8, #123)*

- **`CstText`** — the source-to-`&str` bridge, so a byte-like source materializes a tree when
  it is valid UTF-8 and returns `FinishError::NonUtf8Source` with the offending offset when it
  is not, rather than being unusable.
  — *(R8, #123)*

- **`SeparatedError` gains `Display`.** The D30 separator-name channel was readable only by
  destructuring through `into_components`, so a diagnostic that knew *which* separator was
  involved had no way to say so — half the channel's user-visible point had nothing to render
  through. It is born under the composition rule below, with a single "expected".
  — *(R10)*

- **`BStr` joins the `Equivalent` family.** `utils::cmp`'s module documentation listed `BStr`
  among its backings while the impls were absent from **both** files the family spans, so
  `S: Equivalent<str>` refused `&BStr` with a bare `E0277`. The `Equivalent<str>` /
  `Equivalent<[u8]>` / `Equivalent<BStr>` triple and the `ToEquivalent`/`IntoEquivalent` half
  now exist, gated on `bstr_1` like the `Source` impls.

  The two files are deliberately **not** collapsed into one macro this round: the compile-time
  assert lattice is the anti-drift mechanism, and its rows fail to compile if either half of a
  backing goes missing. That is teeth by construction, where a macro would be organisation-only
  churn.
  — *(R10)*

- **`PolicyComposableEmitter` and `PolicyParseContext`** — a second bundle tier, for the count
  and separator **policy** builders. `ComposableEmitter` covers the collecting combinators at
  their default policy; `at_most` / `bounded` additionally need `TooManyEmitter`, and
  `require_leading` / `require_trailing` need the missing-separator pair. Before this there was
  no bundle carrying those, so a production using a policy builder fell back to spelling the
  ladder — the exact restatement a bundle exists to remove, reappearing one tier up.

  Two tiers rather than one widened bundle, deliberately: widening would make every consumer's
  *concrete instantiation* demand `From<TooMany>` and `From<MissingToken>` on its error type —
  for `Fatal` and `Verbose`, whose impls carry those bounds — whether or not a policy builder
  is ever used. Bundle-1 is a supertrait of bundle-2, so the lattice is strict and each tier is
  true to its own documentation. Pratt is outside both and names its own emitter.

  Additive: bundle-1 is untouched, so nothing migrates. **The re-scoped documentation is the
  other half of the fix** — `ComposableEmitter`'s and `ComposableParseContext`'s docs, and the
  three guide chapters that repeated the claim, said they covered a surface they did not.
  — *(R10)*

- **`Silent`'s `PrattEmitter` impl loses four bounds it never used.** It demanded
  `FromEmitterError` plus `From<UnexpectedEoLhs>` and `From<UnexpectedEoRhs>` on the error type
  while both bodies discard their payloads, which made `Silent` representation-dependent where
  its three siblings are not — `Ignored`'s twin has carried no bounds all along. Removing impl
  bounds only widens the set of admissible types, so nothing migrates; a `Silent<E>` whose `E`
  implements no conversions now drives the token pratt driver. The typed driver's own
  `From<UnexpectedEoLhs/EoRhs>` obligations are unaffected — those are stated at its call
  sites, and `Fatal` and `Verbose` keep their bounds because their bodies use the conversions.
  — *(R10)*

- **`CstProfile` and `KindValidator` no longer require the `rowan` feature.** Both are
  fn-pointer and data only, with no rowan dependence at all, so the gate was placement rather
  than design. It mattered because a `macro_rules!` expansion cannot cfg on tokora's features:
  a macro whose validator arm names `KindValidator` was forcing `rowan` onto every consumer of
  the macro, including those who never build a tree. Strictly more configurations compile; the
  sink half keeps its gate.
  — *(R10)*

- **A CST `Sink` implements `UnclosedEmitter`.** It was the one capability trait the sink did
  not serve, and the omission was not cosmetic: `UnclosedEmitter` is a member of
  `ComposableEmitter`, so a `Sink` could not satisfy the one-bound collecting surface at all —
  a delimited CST parse could not name the bundle its diagnostics-only sibling names. The
  forwarding census could not see it either, because it hard-coded a capability list with the
  same gap; the two omissions hid each other. The census now asserts the membership at the type
  level instead of by string matching.
  — *(R10)*

- **A parse and its emitter can share a source identity.** `Emitter::bound_source` (defaulted
  `None`, so nothing migrates) lets an emitter declare the source it binds; the point at which
  an emitter is attached to an input compares that against the source the parse reads and
  refuses a **provable** mismatch. `Source::REFERENT_IS_BYTES` (defaulted `false`) is how a
  backing says whether an unequal reference proves anything — `true` only for the `?Sized`
  backings, where the reference *is* the data.

  `SourceIdentity` carries **two** things, because there are two failures and they are not
  symmetric. `addr()` is the offset origin, and it is an equality: `&buf[..2]` and `&buf[..3]`
  share one, `&buf[1..]` does not. `extent()` is the byte length, and it is an *ordering* —
  `covers()` is the named relation, and it requires the emitter's extent to be **at least** the
  parse's:

  - a sink bound to a **shorter** slice than the parse reads is refused. It needs no
    out-of-bounds span to do harm: a parser can peek past the sink's end, commit nothing there,
    and let those bytes choose the tree's shape. The result materializes with no
    `SpanOutOfBounds` and no `UncoveredGap`, and round-trips against the sink's own buffer — so
    nothing downstream can see that the structure came from bytes the text does not contain.
  - a sink bound to a **longer** slice is the fixed-arena streaming shape and is accepted. Every
    span the parse emits lies inside the sink's extent, so every slice is correct; the unparsed
    tail is `finish`'s question, answered as `UncoveredGap` or tiled by `finish_partial`.

  See the known limitation below for what this closes and what it leaves.
  — *(R10)*

- **`conformance::emitter`** — the two-assertion kit for an emitter that binds a source:
  `assert_binds_source` catches the author who inherited `bound_source`'s default while
  actually binding one, and `assert_forwards_bound_source` catches a wrapper that forwards
  every emission but not this one. Both failure modes are silent otherwise, and neither is
  detectable from inside tokora.
  — *(R10)*

- **`conformance::cache`** — the cache contract as a runnable kit rather than prose.
  `CacheHarness` drives an implementation through the rollback, put-back and lookahead laws
  the trait states, so a third-party `Cache` can be checked against them instead of
  inferred to comply.
  — *(R9)*

- **`Dialect`** — a two-item anchor (`Lang`, `Lexer`) with the projection aliases `LexerOf`,
  `LangOf`, `DialectSlice`, `DialectInput` and `DialectErrorOf`. Everything else a production
  needs — source, slice, token, span, offset, token capabilities — is reachable by projection,
  so pin those in a one-line subtrait rather than as further associated items.

  There is deliberately no `DialectCtx`. A context bound written through `LangOf<'inp, D>`
  does not unify with one written at the brand: the param-env predicate stays at the
  projection while the obligation normalises. This is documented at the type with the
  mechanism and the verbatim failure, so the shape is not rediscovered by trial.
  — *(W-api-A, #118)*

- **`#[diagnostic::on_unimplemented]` on `FromTokenErrors`, `FromUnclosed` and `Dialect`.**
  A missing conversion now reports what is missing and carries a copyable impl skeleton,
  rather than surfacing as a nested obligation inside crate internals.

  Note for consumers: rustc attaches the *root* obligation's attribute. A bound reached
  through your own blanket-implemented subtrait reports that subtrait, not ours — annotate
  your own dialect traits if you want curated text there.
  — *(W-api-A, #118)*

- **`InputRef::is_exhausted()`** — the consumer-exhaustion predicate: no lexed token is waiting and
  the lexer frontier has reached the end of the buffer. This is the correct gate for a driver loop,
  where `is_eoi()` is not: `is_eoi` answers `true` the moment any lookahead lexes through the end,
  while the tokens that lookahead produced are still unconsumed. `is_exhausted()` is independent of
  the cache implementation.
  — *(R5, #116)*

- **`Cache::RETAINS_FRONT`** — a new trait constant declaring that a front push into an **empty**
  cache always succeeds, i.e. the cache can retain at least one token. It defaults to `false`, so
  the addition is semver-compatible and an existing implementation keeps working unchanged. A cache
  that declares `true` lets the input layer prove its parked-front slot statically unreachable, so
  every probe of it folds away at monomorphization; the shipped caches declare it accordingly
  (`Option` `true`, `GenericArrayDeque<_, N>` `N != 0`, the black holes `false`). A cache that
  declares `true` and then refuses a front push into an empty cache is violating the contract, and
  now panics at the refusal rather than losing the token.
  — *(R5, #116)*

- **Match-first token dispatch.** `select!` / `try_select!` and their runtime,
  `tokora::parser::dispatch_take` / `try_dispatch_take`. One arm per kind, the kind table
  written once beside the patterns, the head classified once against the whole table, and
  each arm receiving the **moved** payload — replacing one annotated closure per kind plus a
  dead re-match, and the hand-written `unreachable!()` for "the kind matched but the variant
  did not" (the runtime turns that into a typed error carrying the table). Both runtimes are
  generic over completeness, so streaming (`Partial`) parsers can use them; the context must
  be a `ComposableParseContext`. The kind expressions must be **const-promotable** — a
  non-promotable one is `E0716` pointing at the invocation rather than the arm; the macro's
  own docs say so and name the fix.
  — *(W-api-B)*

- **Head observation that does not fold a halt into absence.** `InputRef::peek_head_map`,
  `head_satisfies` and `peek_kind`. `peek_one` answers `Ok(None)` for genuine end of input
  **and** for a resource-limit trip or a latched poison boundary, so a production that
  decides on the answer reads a halt as a grammar fact. The new family reserves `Ok(None)`
  for the real end of input and raises the terminal end-of-input error otherwise — marked
  terminal **when your emitter accepts the trip's diagnostic**. A *rejecting* emitter's `Err`
  is built from your lexer's error value and propagates from the fill unmarked, so recovery
  can still spend that trip — `MaybeTerminal`'s doc has that path and the arm it needs.
  `peek_one` is unchanged and is not deprecated.
  — *(W-api-B)*

- **Classify-then-take.** `InputRef::try_expect_take` / `try_expect_take_or_stop`: one
  classification by reference, the commit, then the token **by value** into a projection.
  `try_expect_map` projects by reference, so payloads cannot move out of it.
  — *(W-api-B)*

- **Three-way speculation and an imperative span bracket.** `InputRef::attempt_parse`
  (`Accept` commits, `Decline` rolls back with no fabricated error, `Err` rolls back and
  propagates) and `InputRef::spanning`.
  — *(W-api-B)*

- **Width-1 decisions without a window.** `while_head`, `while_kind` (and their types
  `WhileHead` / `WhileKind`) plus `ParseInput::peek_then_head`. The adapters are `Decision`
  impls pinned at `W = U1`, which is what removes the `::<_, U1>` turbofish, the `Peeked`
  window and the `Ctx` parameter from a hook's signature.
  — *(W-api-B)*

- **Output pinning.** `pinned::<O2, _>(parser)` and `Pinned<P, O>`, for a receiver with
  several output impls (`Collect`, `With`, `FailWith`, `Accepted`) that is ambiguous at
  every downstream site not naming the output. `Pinned`'s phantom is `fn() -> O`, so
  pinning does not move `O`'s auto-traits onto the parser and stays covariant in `O`.
  A fluent `.outputs::<O2>()` method was built and then **removed before release**: it had
  to be a blanket trait method, which plants a candidate on every `Sized` type, and a
  by-value candidate is picked before a consumer's same-named `&self` method. That was
  accepted while it was believed such a collision must be loud; a turbofish with a
  discarded return makes it silent, and an extension method whose job is to pin a type has
  no inferred spelling, so the silent path is its default. A free function resolves by path
  and can shadow nothing. The fluent form stays purely additive later.
  — *(W-api-B)*

- **Three-way ergonomics.** `ParseAttempt::into_option` (the existing `From` conversion, with
  a name that can be found) and `attempt!` — `?` for the three-way vocabulary, early-returning
  `Ok(ParseAttempt::Decline)`.
  — *(W-api-B)*

- **Exclusion parsing.** `Ident::try_parse_except` / `Ident::parse_except`: "a Name, but not
  `on`". The exclusion runs inside the classify predicate, so a decline consumes nothing.
  — *(W-api-B)*

- **Fluent parity** for the free functions most reached for: `ParseInput::{labelled, traced,
  list_until, separated1_by}` and `TryParseInput::opt`. `list_until` and `separated1_by`
  inherit the `Complete` pin of the free `list` / `separated1` they delegate to; that is
  stated on each, with the contrast against `select!`.
  — *(W-api-B)*

- **A declared CST kind space.** `syntax_kinds!` and the module `tokora::cst::kinds`. A
  dialect hand-keeps three things that must agree — the `#[repr(u16)]` enum, the
  declaration-order array its consumers index, and the predicate it hands to `CstProfile` —
  and a disagreement between them is the one shape the sink's own enforcement cannot see,
  because it only ever sees one side. The macro generates all three, plus a checked
  `from_raw`. No feature gate: it names `KindValidator`, which this release un-gates from
  `rowan`, so a `no_std` consumer that cannot enable `rowan` can still declare its kind space
  — verified from a `no_std`, no-`alloc`, no-`rowan` consumer crate on both stable and the
  1.87 MSRV.
  — *(W-api-B)*

- **Emitter diagnostics.** Eight `#[diagnostic::on_unimplemented]` messages across `Emitter`,
  `ComposableEmitter` and the six members between them, so a type short of the bundle is told
  which member is missing rather than getting rustc's default trait-bound phrasing.
  — *(W-api-B)*

66. **`cast::token_any` and `cast::tokens` close the two gaps in the cast module's token
   helpers.** `token` answers one kind, first match, and that was the whole vocabulary: a node
   whose grammar puts more than one token kind in the same slot, or repeats one token kind under
   a parent that gives it no node kind of its own, had nothing to reach for and had to hand-roll
   the same `children_with_tokens` filter at the call site.

   `token_any` is the first direct token child whose kind is one of several, scanned in
   **document order, not `kinds` order**: trying `kinds[0]` and falling back to `kinds[1]` would
   answer the wrong token for a node carrying both, which is a difference no caller should have
   to know about. `tokens` is every direct token child of one kind, in document order, returned
   through the new `cast::TokenChildren` iterator — `cast::children`'s `NodeChildren` is an
   alias for an upstream rowan type, and rowan has no ready-made iterator over one token kind,
   so this crate now provides one.

   Both are generic over `Language` and take their kind(s) by reference, matching `token`'s own
   shape; `token_any` takes `&[L::Kind]` rather than a predicate closure because callers pass a
   fixed, short, statically-known set of alternatives, not an open-ended rule. Both also see
   only *direct* token children, exactly like `token`: a token belonging to a child node is that
   node's, and neither helper reaches into it — covered alongside the document-order and
   empty/no-match cases in `cst::cast`'s test suite.

### Fixed

- **An `Input` is bound to exactly one emitter, at construction, for its whole life.** The input
  stores two **suppression** watermarks whose meaning is relative to *one emitter's log* —
  `emitted_error_end` (lexer-error dedup, shipped) and `front_reported_end` (close-miss
  suppression, added this release) — while `Input::as_ref` took the emitter as a **parameter**,
  so which log a watermark described was chosen per borrow. A watermark carried into a different
  log claims a report that log never received, and because these watermarks suppress, the
  consequence is a **silently dropped diagnostic**, not a duplicated one.

  It was latent: no in-crate path paired one input with two emitters, and `Input` is
  `pub(crate)` and unexported, so no external path existed. That is a property of in-crate
  discipline, though, not of the type system — which is what this replaces. `Input` now owns an
  `emitter: Ctx::Emitter`; `with_state_and_cache` becomes `with_state_and_context`, taking
  exactly the `InputContext` that `ParseContext::provide` already returns; and `as_ref` loses its
  emitter argument. The per-borrow choice is not merely unexercised, it is **unconstructable**.

  Three consequences worth naming. The **source-identity handshake** moves into the constructor,
  which is where the pair now forms — so it runs once per input rather than once per handle, and
  covers every borrow by construction. `impl Clone for Input` is **deleted**: a clone would have
  had to duplicate an emission log, which is not a thing a log can be. And the `{emitter mark,
  emitted_error_end, front_reported_end, poison_boundary}` group that `Checkpoint` has always
  saved and restored as a unit is now a unit in the *live* structure too — before, two of the
  four were facts about a log the input did not hold.

  **The `as_ref` call-site census from #127 is deleted in the same change**, and the reason is
  the point: that rail guarded the claim *"no in-crate path pairs one `Input` with two different
  emitters"* during the window when the claim needed guarding. There is no longer an emitter
  argument for a new call site to mispair, so the claim is unconstructable rather than merely
  unviolated, and the rail has nothing left that could fail. A rail whose property becomes
  structural dies with the property rather than lingering as a check that can no longer check —
  the *check that stops being a check* class this campaign catalogued. Its replacement is the
  type system. One cell arrives in its place, `dedup_watermark_survives_a_reborrow_of_one_input`,
  which pins the only shape still *writable*: a watermark raised in one handle still suppresses
  the next handle's re-lex of the same region. That cell is also what forecloses the cheaper
  remedy the issue weighed — clearing every log-dependent watermark on borrow — from being
  reintroduced later as a tidy-up, since under it the cell reports twice.

  No public item changes signature: `Input` is `pub(crate)`, and `InputRef` — which *is*
  re-exported, from both the crate root and the prelude — is untouched, as are `Session`,
  `Checkpoint`, the scan paths, transactions and session points.
  — *(R11, #128)*

- **The documentation builds at every feature point.** `cargo doc --no-default-features` had
  never been run: it exited 101 with `error: could not document tokora` and **39 unresolved
  intra-doc links** across 15 files. The crate denies `rustdoc::broken_intra_doc_links` via
  `[lints] workspace = true`, so this was a hard error rather than a warning — but the only
  doc job built `--all-features`, where every gated target exists and therefore every link
  resolves. The blind spot was structural: an all-features doc build cannot see this class.

  Each affected link now carries a per-configuration pair — the gated arm is byte-identical
  to what shipped, the ungated arm renders the same sentence with the unavailable item as
  plain code text rather than a dead link. The targets are all gated `any(std, alloc)`
  (`Verbose` and its family, `StackedTransaction`, the session-point and stacked-transaction
  entry points, `foldrn`, `separated1`), so both configurations render clean prose. No
  crate- or module-level `allow` was added: an exemption whose subject is "all broken links
  here" would silently absorb the next genuinely broken one.
  — *(R11)*

- **Documented what the dispatch tables actually do on a malformed table.** `DispatchOnKind`
  and `FusedDispatchOnKind` resolve duplicate kinds **first-wins** (the lookup stops at the
  first match, so a later duplicate is dead), and an oversized table is checked only by a
  `debug_assert!` — in release, an entry past the branch range can never select a branch and a
  kind found only there classifies as a dispatch miss. Neither is rejected at construction,
  deliberately: refusing at build time would turn a misuse into a behaviour change. Nothing
  moved; the docs now say what the code does. The non-fused constructor also gained the fused
  twin's "or a latched limit boundary" parenthetical on its end-of-input row.
  — *(R11)*

- **Documented the token-level Pratt driver's recovery posture.** When a prefix operator's
  operand never arrives, the driver reports the diagnostic and then returns **the operator
  token itself** as the expression; an infix operator missing its right operand yields the LHS
  alone. Under a recording emitter the parse continues carrying that stand-in. The token driver
  has no node to withhold, so this is deliberate — callers wanting the stricter posture use the
  typed driver — but it was written down nowhere and had no cell. Both exits are now documented
  and the prefix exit is pinned.
  — *(R11)*

- **Corrected where `finish_partial` places a trailing gap.** The tiling comment said the tail
  tiles "into the root". Measured: the tail is tiled **before** the open frames are closed, so
  when the stream ends with a node still open the gap becomes a child of the **innermost open
  node**. Under `finish` the question cannot arise — an unbalanced stream is refused — so the
  old sentence was true only for the case `finish_partial` exists to relax. `tree.text() ==
  source` holds either way; it is the placement that differs, and tooling that walks by node
  rather than by text can see it. Behaviour unchanged; the construct now has its first
  placement pin, which is why the sentence was unverifiable for as long as it was.
  — *(R11)*

- **The CAS-less `no_std` source-backend rows say what they require.** Four rows in the feature
  reference read "as the dep allows". `bytes`, `hipstr` and `smol_bytes` reach a refcounted
  buffer through `Arc`-shaped sharing and need atomic compare-and-swap, so they cannot build on
  a target like `thumbv6m-none-eabi`; `bstr` is the CAS-free choice and is pinned by a CI cell
  on that exact target. The CAS-needing three are documented rather than pinned as
  expected-failures, since an expected-failure cell would go green for the wrong reason the day
  an upstream crate gained a CAS-free fallback.
  — *(R11)*

- **A session's budget guard now holds in release builds.** `PartialSession` refuses an
  attempt that would exceed its budget or that follows a terminal latch, and it relies on the
  downstream `From<SessionRefusal>` conversion preserving terminal status. That requirement
  was enforced by a `debug_assert!`, so a **release** build could surface `BudgetExhausted` or
  `TerminalLatched` as an error whose `is_incomplete()` returned true — and a caller would
  then refill and retry forever against a session that refuses before doing any work. A
  zero-work infinite refill loop is precisely the denial-of-service shape the budget exists to
  prevent, and it was absent from the builds that matter. The check is now unconditional.

  **Behaviour change:** a `From<SessionRefusal>` implementation that drops terminal status now
  fails in release as it always did in debug. The cost is one `is_terminal()` call per
  *refused* attempt — never on the success path, and nothing per token or per event.
  — *(R8, #123)*

- The `cache` module no longer advertises a dynamic, allocator-backed cache with unlimited
  lookahead. No such implementation exists, `alloc` does not add one, and none could serve a
  public peek past 32 tokens — `Window` is sealed at U1–U32. Lookahead beyond the window is
  what transactions are for.
  — *(R9)*

- **A regime change could silently corrupt lexer-error deduplication.** `set_state` and
  `state_mut` share a body that cleared the cache, dropped the parked token and
  cleared the poison boundary, **then** cloned an offset, **then** re-anchored the dedup watermark.
  `Offset::clone` is caller code. A panic there left the watermark holding the **dead regime's**
  value on an input that had just been unpoisoned — so for the rest of the parse, lexer-error
  dedup compared new-regime spans against a stale mark and **silently suppressed or duplicated
  diagnostics**.

  The read is now hoisted above every mutation and takes the span's end directly, which is provably
  the same value: the cursor reports the parked token, else the cache front, else the span — and the
  two clears exist precisely to empty the first two, so the fallback branch was the one that ran
  either way. The old "cleared before the cursor read" ordering was documenting a coupling that
  existed only to steer the cursor into that branch.
  — *(R9)*

- **A checkpoint rollback is all-or-nothing.** `restore_unchecked` interleaved caller code with
  the facts it was installing: cache eviction,
  session-point abandonment and a span clamp all ran between the lineage pop and the position
  restore. A panic in any of them left the input **half-rolled-back** — lineage, cache and emitter
  on the checkpoint's branch while the position and the remaining facts stayed on the abandoned one.

  The body is now two phases. **Phase 1 runs every operation that can execute caller code, while
  the input is still wholly on the branch being abandoned** — so a panic there leaves it there,
  which is the *unchanged* half of all-or-nothing. **Phase 2 installs every fact with no caller
  code among them.** (Staging the evicted values to a single drop site was the obvious repair and
  is not available: checkpoints work in allocator-less builds, so there is nowhere to put an
  unbounded number of evicted entries. The ordering achieves the same guarantee without one.)

  **The residue, stated precisely:** a rollback is atomic unless a **cached token's, or an abandoned
  session point's, own `Span`/`State` drop panics** — unreachable for every type this crate ships,
  since they are `Copy`. Such a panic leaves the input on the branch it was already on, with part of
  a cache that would have been discarded anyway.
  — *(R9)*

- **An interrupted restore can no longer make an abandoned scan's diagnostics permanent.**
  `restore_entry` restored the input's position and facts and **then** cloned a cursor to hand
  `Emitter::rewind`. `Offset::clone` is caller code, so a panic there left the input restored with
  the emitter mark **not rewound** — and the scan scope's fallback then *released* that mark, making
  an abandoned scan's diagnostics and token-observer effects permanent while the parser retried from
  the restored position.

  **Hoisting the clone within the body does not fix this**, which is worth stating because it is the
  obvious repair: wherever in `restore_entry` the clone sits, failing it still skips the rewind. The
  operation had to leave the body entirely. The scan's entry record now carries the rewind cursor,
  cloned at capture — **before the mark exists** — so `restore_entry` performs no caller operations
  at all.

  The same round removed three further in-place field writes on the restore paths where a panic
  would have left the input and the checkpoint disagreeing about the dedup watermark and the poison
  latch.

  — *(R9)*

- **A token left unconsumed by a `to`-shaped scan stop or by a `try_expect*` / `probe_close`
  decline is now retained even when the configured `Cache` refuses it**, so the cursor, offset,
  `is_eoi`, the `sync_balanced` hole anchor and the lexer-state tally after such a stop are the
  same under every cache capacity — including a zero-capacity `()`, where the token used to be
  dropped and re-lexed by the next call. `ClosePayload::CacheFront` is renamed
  `ClosePayload::AtFront` (crate-internal).
  — *(R5, #116)*

- **`InputRef::lexer()` now resumes under the state that produced the newest retained token**
  rather than under the committed state, so a widening lookahead no longer lexes the token after
  a retained run as if that run had never been scanned. A by-value `Lexer::State` limiter is no
  longer bypassed by the lookahead pattern, and the tokens a drain yields, the final state and
  the diagnostics it emits no longer depend on how deep — or in how many steps — the caller
  peeked first.
  — *(R5, #116)*

- **A non-final EOF now reports the lexer's own end offset** instead of the end of the last item
  it yielded, so bytes the lexer skipped before exhaustion (trailing whitespace, a comment tail)
  are no longer omitted from the `Incomplete` frontier a refill driver reads. The reported offset
  is floored by what was already lexed and clamped to the buffer.
  — *(R5, #116)*

- **`InputRef::foldr_within` no longer drops a run shorter than its window.** The exhaustion
  arm returned the untouched initializer *after* consuming the run into the fold's buffer, so
  every run of length `1 ..= W::CAPACITY - 1` was silently discarded on the `Ok` path. Both
  exits now converge on the single reverse drain, as the sibling `foldrn` already did.
  — *(R7, #117)*

- **A render-freeze suite.** One pinned `Debug` and `Display` string per public error type
  (`tests/render_freeze.rs`). Rendered text is public API that no signature describes and no
  tool reads: `cargo semver-checks` compares shapes, and a derived `Debug` that gains a field
  has the shape it always had. That gap cost this crate twice — `UnexpectedEnd`'s undisclosed
  `terminal` field, and a doubled "expected" that survived in the one carrier nobody had
  pinned. A failure here does not mean something broke; it means a rendered surface moved and
  has to be disclosed, and blessing it is a reviewed diff of the exact bytes a consumer's
  frozen fixtures will see.
  — *(R10)*

- **The `no_std` tiers now execute in CI, and the first run of that cell was red.**
  `cargo build --no-default-features` gated a production-code violation but never compiled a
  test target, so nothing `no_std`-shaped had ever *run* — a whole test suite in a configuration
  that executed none of it. Enabling it immediately found a dead-code failure (`scan_tick` has no call
  site without an allocator, since its callers are allocator-gated) that predates this round
  and that no gate could see. Fixed, and the two host runs plus D46b's positive
  `thumbv6m` + `bstr` cell are now CI steps.
  — *(R10)*

- **The input's three monotone witness counters are checked, and loud at the wrap point.**
  `next_ckp_id`, `savepoint_seq` and `cache_pushes` incremented with a bare `+= 1`. The stakes
  are higher than bookkeeping: the checkpoint ids are what make the live-checkpoint list
  sorted-ascending, and this release's `contains` / `pop_through` fast paths are binary searches
  *licensed by* that sortedness — a wrapped id violates it with nothing to say so. Each
  increment now panics by name instead of rolling over, following the CST sink's own witness
  counter. 2^64 is not a practical horizon; the point is that a shipped optimization's
  precondition cannot break in silence.
  — *(R10)*

- **The session-point abandonment law is written where users read it.** An abandoned point
  commits, and the campaign carried that to the end as an open *policy decision* between
  commit, rollback and a hybrid. Two of the three are not buildable: `Session::drop` holds the
  lineage and the emitter but not the input, so it cannot restore span, state or cursor —
  "rollback on abandon" is unwritable from that site, and the one thing that is writable
  (rewinding emissions while keeping the position) is the torn state the settle discipline
  exists to prevent. A drop-time `debug_assert` is refused too: abandonment via `?` through an
  enclosing rollback is a legal history. Documented, not changed.
  — *(R10)*

- **`commit_probed`'s temporal contract is enforced in debug builds.** A close payload is valid
  only while the committed cursor sits where `probe_close` left it — everything in the gap,
  including the end-state pass the deferred drivers run there, must be cursor-neutral. That
  lived in prose. The payload now carries the probe-time committed position behind
  `debug_assertions` and the commit checks it, so an edit that consumes or rewinds in the gap
  names itself instead of settling the wrong token. `ClosePayload` is crate-internal, so no
  public surface moves.
  — *(R10)*

- **`Fatal`'s `Default` and `Debug` are `Lang`-generic**, like its `Clone`/`Copy` and like
  `Silent`'s twins. They were written `impl<T: ?Sized> ... for Fatal<T>` — `Fatal<T, ()>` only —
  so `Fatal::<E, MyLang>::default()` did not resolve and a branded `Fatal` could not be printed
  at all. Additive: impls widen, and the render at `Lang = ()` is unchanged.
  — *(R10)*

- **A transposed compile-assert now pins something.** `Ignored`'s no-op `SeparatedEmitter`
  assert instantiated the trait as `<'a, Sep, L>` — separator in the lexer slot, lexer in the
  language slot — and compiled anyway, because `Ignored` implements the trait for any parameters
  at all. An assert over an unconstrained type type-checks whatever you hand it. Rewritten at
  the real signature and exercised at both an unbranded and a branded language; transposing the
  slots back now fails the build.
  — *(R10)*

- **`Emitter::emit_lexer_error`'s contract says what an `Err` return means.** Under a rejecting
  emitter the `Err` **is** the delivery: the input layer advances its dedup watermark *before*
  calling, so a later scan over the same region will not offer the diagnostic again. The
  natural reading is the opposite one, which is why it is now written down at the trait and at
  `Fatal`'s impl.
  — *(R10)*

- **Documentation that was false, corrected rather than softened.** Four `find_boundary` docs
  claimed the result "is always a valid slice position" *and* that indices at or beyond the end
  come back unchanged — which cannot both hold, since `len + 1` returned unchanged is not a
  slice position. They now state the rounding direction (**down**), the in-range guarantee, and
  the out-of-range behaviour separately, and the trait-level doc gains the word "down" it was
  missing. `Transaction`'s "the guard is two words" is replaced by the measured truth: the
  guard embeds a whole `Checkpoint` — cursor, span, `L::State`, two `u64` marks, poison
  boundary — 88 bytes with a near-stateless lexer and `O(size_of::<L::State>())` in general.
  Its op-count claims were true and are kept. `utils::cmp`'s backing list now names `BStr` and
  the `smol_bytes` family, which it implements.
  — *(R10)*

- **`UnclosedEmitter::emit_unclosed`'s `FromUnclosed` bound placement is recorded as chosen.**
  It sits on the method rather than on the trait so an emitter that never routes an unclosed
  pair pays nothing; hoisting it would force every implementor's error type to carry the
  conversion unconditionally. Documented so a later cleanup does not "fix" it.
  — *(R10)*

- **The collection gate's own census could pass by not looking.** `parser::many`'s `GATE_CENSUS`
  read four hard-coded sources and counted one exact spelling of the swallow beneath the gate —
  `emit_error(Spanned::new(span,`. A swallow spelled any other way, or written in a fifth source,
  moved neither tally, so the equality it asserted still held and the census stayed green over an
  ungated emit-and-continue site. A census quoted as evidence that answers by not looking is worse
  than none.

  The swallow is now a single chokepoint — one `emit_error` call in the whole `many`/`fold` tree,
  with the three never-recoverable witnesses above it — so a driver physically has nothing to spell
  differently. The census matches the **call** rather than its arguments and asserts the count is
  zero in every driver source; it scans the chokepoint's own body for the three witnesses ahead of
  the emission; and a third test requires every module of the driver trees to be classified, so the
  source list cannot fall behind the tree. Every scan panics when the thing it looks for is absent
  instead of finding nothing to check. Shown non-vacuous by planting a swallow in a fifth source
  with a spelling the old shape did not match — it fails, naming the file, then the plant was
  removed. No behaviour change. — *(#148, verification debt)*

  **And the same defect, one level in: the witness scan proved presence, not gating.** The scan
  above requires each of the three never-recoverable witnesses to appear once in the source ahead
  of the emission. Textual presence ahead of a call is not control-flow domination — a body that
  reads all three into `let _ =` bindings and then emits unconditionally satisfies it with the gate
  entirely gone. Compiled and run: the census stayed green.

  Answered by mutation rather than by a stronger scan, since proving domination from source text
  means writing a Rust parser inside a test module that must also build under
  `--no-default-features`. Each witness was neutered in turn and the whole `--all-features` suite
  run: the frontier-`Incomplete` witness and the descent-trip witness each red tests — and
  **`at_committed_boundary()` red nothing at all**, in the entire suite. Its only cell was negative
  (a boundary the cursor has *not* reached must not be charged to an ordinary failure), which a
  deleted witness also satisfies. Unguarded, a collection that runs onto a poison boundary files
  the stop as an ordinary syntax error and returns a **silently truncated success** — `Ok` over
  input the scanner never read.

  `tokora/tests/collection_terminal_stop.rs` gains the positive direction: five `r1b_*` cells, one
  per try-driven family plus the truncation case, each with the file's non-vacuity control (the
  same probe run twice with only the scan limit changed, required to disagree, and required to have
  actually tripped). All five are red under the mutation and green without it. The census now states
  what a needle scan proves and names the suite that proves the rest, and both it and the
  chokepoint's own docs carry the re-check procedure. No behaviour change. — *(#148, verification
  debt)*

- **The recursion-limit test suite's stack-address witness made every Miri job and the ASan job
  red, on `main`.** `pratt_limit_unit_sink`'s unwind cell corroborates its two depth-cell
  assertions out-of-band by comparing the addresses of two stack locals. That comparison measures
  frame liveness only while the addresses are native stack offsets: ASan can relocate a frame's
  locals onto a heap-allocated fake stack and Miri hands out virtual allocation addresses. Three of
  four instrumented hosts measured the *unwound* frame as farther from the baseline than the
  descent it had already left — inverted operands, so no threshold rescues the comparison — while a
  fourth (ASan on aarch64-darwin) passed on the same source. A relation whose answer changes with
  the runner is reporting the runner, so it is now scoped to native builds via `cfg!(miri)` plus a
  `TOKORA_SANITIZER` variable `ci/sanitizer.sh` exports for every leg it runs.

  The two depth-cell assertions stay **active** under Miri and every sanitizer, and the gated one
  is not replaced by a second library-side counter: reading the same cell twice is not
  corroboration. A skipped assertion announces itself on the process's real stderr — not through
  `eprintln!`, which libtest swallows for a passing test — so the one build where the check does
  not run is not the one build where nobody can see that. — *(#148, verification debt)*

- **`pratt_limit`'s deep-stack wall-clock bound was calibrated for the wrong machine, and it made
  the `miri-tb-x86_64-unknown-linux-gnu` leg intermittently red on `main`.** `on_a_deep_stack`
  bounds every deep-recursion cell to a fixed number of seconds so a recursion-limit regression
  fails the test with a message instead of hanging the process. Under interpretation that number
  measures the interpreter, not the parser, and not by a flat factor: `-Zmiri-tree-borrows`
  revalidates a growing borrow tree on every access, so
  `the_default_budget_refuses_a_deeper_chain_and_unlimited_restores_it`'s 1000-level `unlimited()`
  chain — this file's deepest cell — scales worse than linearly with depth, not by the roughly two
  orders of magnitude a flat per-step slowdown would predict. Measured directly, with the exact
  command and `-tb` flags `ci/miri_tb.sh` runs: 60–69s across three runs on aarch64-apple-darwin —
  already more than half of the native 120s bound, on a different host and architecture than the
  `x86_64-unknown-linux-gnu` shared runner CI failed on, which is why that leg flaked instead of
  failing outright every time.

  The bound is now `cfg!(miri)`-scoped: 700 seconds under Miri — a ×10 margin over the slower
  reading, sized for the cross-host and shared-runner gap CI had already demonstrated, not just
  this machine's own run-to-run noise — and 120 seconds, unchanged, natively. The bound's purpose
  is untouched: a genuine hang is still caught, on a schedule that fits the machine actually
  running it, and the native path carries no behaviour change — an unlimited native run finishes
  in hundredths of a second, nowhere near either number. — *(#148, verification debt)*

- **The name-collision gate reported an incomplete verdict as if it were a result.**
  `gen_probe.py` had no template for the owners #147 introduced, so every row on them came back
  `FATAL`. Templates alone were not enough: a probe naming an owner the same diff introduces cannot
  compile against the base ref, and base-no-compile was unconditionally `INCONCL`. A new
  `new-owner` verdict says exactly that, and carries two witnesses so it cannot be manufactured —
  rustc must name the owner as unresolved on base and must not on head. The verdict is complete
  now: 27/27 rows probed, 0 FATAL, 0 INCONCL, 0 UNPROBED. — *(#148, verification debt)*

- **That verdict proved base was broken and never checked that head ran.** Its two witnesses
  required rustc to name the owner unresolved on base and to stay silent about it on head — but
  silence is also what a head build broken for an unrelated reason looks like: `no-compile`,
  `upstream-fail`, `bad-witness(...)` and `unreached` none say the owner is unresolved either, so a
  head that failed to compile the probe for a reason having nothing to do with the owner — or that
  never reached the call at all — still read as proof the owner is new. `new-owner` now
  additionally requires head to show a completed run, checked by an allowlist of the one shape
  that is evidence (`witness=*`) rather than a denylist of the shapes already known to be broken,
  which is silent about the next one nobody has named yet. Re-run against a genuine pre-#147 base,
  9 of the 14 `RecursionLimitReached` rows the prior entry counted turn out to have been passing on
  exactly this hole: `map_offset` and `of`'s templates call the real method with fewer arguments
  than it takes, and the other five methods' `used` spelling forces a `u8` return type none of them
  have, so head never compiled on any of the nine. They now score `INCONCL`, correctly — only the
  five methods whose `discarded` spelling silently and successfully calls the real inherent method
  were ever provable, and the fix does not widen to paper over the rest. — *(#148, verification
  debt)*

- **That fix's own allowlist was still too wide, and a sibling verdict had none at all.**
  `new-owner`'s `case "$h" in witness=*)` accepts `witness=1` exactly as readily as `witness=0` —
  but the marker is CONSUMER-CALLS, attribution rather than existence (see the comment where it is
  assigned), and `witness=1` means the CONSUMER's own extension item took the call, i.e. the
  probed name on the new owner was never a candidate this run. A template that compiles clean
  while constructing no real collision would still have scored `new-owner`. The allowlist now
  requires `witness=0*` specifically — the one shape that says tokora's item won the resolution —
  and a `witness=1` head reads `INCONCL`, naming the reason.

  Separately, the `glob-err`/`glob-ok` verdicts intercepted only the LITERAL STRING `no-compile` on
  the base side before trusting head's evidence. A base broken for any other reason —
  `upstream-fail` chief among them — fell through into the head-only checks and let a head-side
  ambiguity decide the verdict alone, with no valid before-state at all. The base side is now
  checked against an allowlist too: `no-compile` or `unreached` (the only two shapes a
  compiling-or-rejected glob probe can produce, since `glob_name`/`glob_macro`'s `drive()` calls
  only `ran()` and never `reached()`), anything else `INCONCL`.

  Both proved in the failing direction, against a real glob collision and a real `new-owner` row,
  then reverted — `git diff` confirmed clean before this fix was committed. A forced
  `base=upstream-fail` alongside a genuine head-side ambiguity moved the glob row from `glob-err`
  to `INCONCL`; a forced `witness=1` (explicit UFCS on the consumer trait, bypassing inherent-method
  resolution) on a real `new-owner` row moved it from `new-owner` to `INCONCL`. Re-run against the
  same pre-#147 base as the prior entry, the tally is unchanged — 5 `new-owner`, 9 `INCONCL` —
  because none of the 14 real rows there naturally produce `witness=1`; only the forced case
  exercises the new branch. — *(#148, verification debt)*

- **That base-side allowlist had a mirror-image hole on the head side, and it was the more
  exploitable of the two.** After the fix above, `glob-ok`'s only remaining check was
  `elif [ -n "$said" ]` — accept once rustc says *something* ambiguity-shaped anywhere in the
  head log, with no check on `$h` itself. `upstream-fail`, `bad-witness(...)` and any
  `witness=*` all read as a pass exactly as readily as a genuine `unreached`, so a head build
  broken for a reason having nothing to do with the probed name — provided an attributed
  ambiguity diagnostic happened to be sitting in the same log — still scored `glob-ok`. That is
  acceptance on the absence of a completed head-side probe: the same defect class the base-side
  fix above had just closed, on the other side of the same row.

  `h` is now held to the identical two-shape allowlist as `b`: `no-compile` or `unreached`,
  because `glob_name`/`glob_macro`'s `drive()` calls only `ran()` and never `reached()` — a head
  that actually finishes compiling and running can only ever read `unreached` here, never
  `witness=*`. `glob-ok` now requires `h = unreached` specifically; `upstream-fail`,
  `bad-witness(...)` and any `witness=*` read `INCONCL`, naming the reason, instead of falling
  through.

  Proved in the failing direction against the same pre-#147 base and the same
  `RecursionLimitReached` glob row as the prior entry: a forced `head=upstream-fail` alongside
  the row's genuine, untouched ambiguity diagnostic moved it from `glob-ok` to `INCONCL`; forcing
  `bad-witness(calls=2,reached=0)`, `witness=0` and `witness=1` the same way each land `INCONCL`
  too. Reverted afterward; `git diff` confirmed clean before this fix was committed. Re-run
  unperturbed, the same row reaches its natural `glob-err` verdict, unchanged from before this
  fix.

  Audited the rest of the file for the same asymmetry — a verdict proving one side reached a
  conclusion without proving the other did. `new-owner`'s two witnesses already check
  `base_unresolved` and `head_unresolved` by the same predicate, and the item-row ladder's
  `unreached`/`upstream-fail`/`bad-witness` filter matches against `"$b$h"` concatenated, which
  catches either side by construction; the base-only `case "$b" in witness=1*|no-compile)`
  further down is not a shape gap of this kind; it fixes the pre-release baseline's expected
  value, and every value `$h` can still hold past it is already handled by name in the branches
  below. This glob-row pair was the only place in the file carrying an allowlist on one side and
  none on the other. — *(#148, verification debt)*

- **`gen_probe.py`'s two newest templates assumed a call shape neither real method has, so 9 of
  the 14 rows the entry above counted could only ever read `INCONCL`.**
  `error_subject_method`/`error_subject_assoc_fn` — added for
  `RecursionLimitReached`/`NonAssociativeChain` — rode the same fixed zero-argument, `-> u8`
  consumer shape every other inherent-method/assoc-fn template does. That shape fits neither
  type: `map_offset` consumes `self` and takes a closure, `of` takes the type's own fields (two
  arguments on `RecursionLimitReached`, one on `NonAssociativeChain`), and none of the five plain
  accessors (`offset`, `offset_ref`, `exceeded`, `depth`, `limitation`) returns `u8`. The emitted
  call failed to compile on the head side before rustc ever reached the collision —
  `map_offset`/`of` with E0061 (too few arguments), the other five's `used` spelling with E0308
  (the `let`-binding's forced type) — which is exactly the hole the entry above named and left
  open: "the fix does not widen to paper over the rest."

  Both templates now derive the call's arity, and the `used` spelling's `let`-binding type, from
  the REAL signature instead of assuming one. Verified two-sided, against the same genuine
  pre-#147 base the entries above use (`60f27a3`): before this fix the 14 `RecursionLimitReached`
  rows scored 5 `new-owner` / 9 `INCONCL`, matching the count already on record; after, all 14
  score `new-owner`, each on a head run that compiled and completed (`CONSUMER-CALLS: 0` —
  tokora's own item took every call). No effect on this branch's own run: it adds no name
  reusing either owner, so it scores `probes=0`/`PASS` regardless — the fix matters the next
  time a PR gives one of these two types a genuinely new item. — *(#148, verification debt)*



55. **A `trace`-feature preview no longer reports a partial `Debug` rendering as the complete,
 short value it is not.** `bounded_debug` — the bounded sink `trace_preview` (behind the
 `trace` feature) uses to build its per-event source preview — discarded the `Result` from its
 own `write!` call outright, so its truncation flag came from the sink's private bookkeeping
 alone: `true` only when the sink itself had refused a write after filling its 24-character
 window. A `Debug` impl can fail for a reason that has nothing to do with the sink —
 `fmt::Result` is a plain `Result`, and neither the trait nor `Slice`'s bound (`PartialEq + Eq
 + Debug`) forbids it — and that case left the flag `false`: the preview then rendered
 whatever partial content had been written, with no ellipsis, as though it were the whole
 value.

 The single flag is replaced with a three-state outcome — `Complete`, `WindowTruncated`, or
 `FormatFailed` — because a bool cannot carry both facts a reader needs kept apart: whether
 there is more of the value than the window shows, and whether the value's own `Debug` impl
 finished at all. Folding a foreign `Err` into the same flag a real truncation sets is just as
 wrong in the other direction: a `Debug` impl can write its complete, short rendering and only
 then fail for a reason of its own, and reporting that as truncated appends `…` to a trace
 line for a value that has no more content, claiming a continuation that does not exist.

 The sink's own bookkeeping (`sink.truncated`) is what tells the two causes apart, and does so
 reliably: the sink has exactly one branch that ever returns `Err`, and that branch sets
 `sink.truncated` in the same statement, so a `write!` failure the sink did not record cannot
 be the sink's. `trace_preview` now renders each state distinctly — no marker for `Complete`,
 `…` for `WindowTruncated`, and a separate `<fmt error>` marker for `FormatFailed` that says
 the renderer broke without claiming missing content. This is a correctness fix, not a
 performance one: it changes output only for a `Debug` impl that fails on its own account,
 which no in-tree `Source`/`Slice` pairing does, so every case that already reached the sink
 normally — which is to say, every case this crate has ever exercised — renders
 byte-identically to before.

56. **A recursion-limit trip an element ANSWERED was still spendable — as a successful, complete
   collection.** Item 48 above closes the path where an element hands its trip back as `Err`: the
   collection drivers gate that at one chokepoint and re-raise it. They gated **only** that path.
   An element that catches [`RecursionLimitReached`](https://docs.rs/tokora/latest/tokora/error/struct.RecursionLimitReached.html)
   itself and then reports *no more elements* — by declining, or by accepting without consuming
   anything — hands the driver an `Ok`, the chokepoint never runs, and the driver's **absence** exit
   reads it as the ordinary end of the construct. `repeated()`, `separated_by(..)` and both
   delimited forms returned `Ok` with everything collected so far, filing nothing. A resource
   budget stopped the parse and the parse reported a complete construct — the same defect as item 48,
   for every error type equally, reached through the exit item 48's gate does not cover.

   The absence exits now consult the same session counter, at the same per-element granularity, and
   through a second chokepoint of their own: nine exits across the four drivers — the element
   decline, the no-progress stall, and in the delimited pair the close probe's `WrongToken` and
   `Eof` arms — surface the stop as the terminal-marked end-of-input error those exits already
   produced for a *scanner* stop, instead of ending the construct cleanly.

   | driving an element that catches a depth trip and then reports absence | before | now |
   |---|---|---|
   | `repeated()`, `separated_by(..)`, and both delimited forms | `Ok` with the elements collected before the stop, nothing filed | `Err` — a terminal end-of-input, still nothing filed |

   **Unchanged here, and deliberately so**: the *scanner* half of this gate stays off an exit
   resting on a **real token**. A `Close` verdict from the delimited drivers' close probe, and the
   mid-scan closer in the delimited separated driver, read a committed pre-trip token, so the
   construct ended *ahead of* any boundary a later lookahead latched and a wider scan window parses
   the identical source to the identical value. The *descent* half of it is a different kind of fact
   and does belong on those exits — item 57 below is that correction. An `Accept` is untouched either
   way, and stays that way: an element that catches a trip and still returns a value has answered
   it, not concluded absence, so the driver is faithfully collecting what it was handed rather than
   manufacturing a stop of its own — item 48 above states this as the fourth channel a caught trip
   can still spend, and why gating it would be a broader contract than #148 establishes. And
   the granularity floor item 48 describes is exactly the same here — the baseline is one
   **element**, so a trip an *earlier* element caught and parsed past does not end the collection
   when a later one legitimately runs out of input.

   The eight `*_while` drivers and folds shared this hole; **item 58 below closes it** for them, and
   the measurement that found it is recorded there. — *(#148 R7)*

57. **A closer that arrives after the trip does not unmake it.** Item 56 gated the delimited drivers'
   *absence* exits and left the arm where the closer is genuinely present ungated, on the reasoning
   that a committed pre-trip closer settles the question. It settles **one** of the two questions,
   and the two are facts of different kinds:

   - a terminal **scanner** stop is a fact about a token *position*. The close probe is cache-first,
     so a `Close` verdict rests on a real pre-trip token: the construct ended *ahead of* whatever
     boundary the element's lookahead went on to latch, and that boundary is not about it. Reading it
     there would fail a parse a wider scan window completes to the identical value, so it still does
     not — unchanged from item 56;
   - a **descent** budget trip is a *counter event that already happened inside the element attempt*.
     Nothing arriving afterwards unmakes it. An element that caught a
     [`RecursionLimitReached`](https://docs.rs/tokora/latest/tokora/error/struct.RecursionLimitReached.html),
     reported *no more elements*, and was then followed by a real closer produced a **successfully
     closed collection that had silently spent a resource-limit stop** — item 56's defect, through the
     one arm item 56's gate deliberately does not cover.

   Three exits now consult the trip counter before they commit a real closer, at the same
   per-element granularity: `repeated().delimited()`'s decline arm and its no-progress epilogue, and
   `separated_by(..).delimited()`'s epilogue.

   | driving an element that catches a depth trip, reports absence, and IS followed by the closer | before | now |
   |---|---|---|
   | `repeated().delimited()` over `"( 1 2 3 )"`, element declines | `Ok([1, 2, 3])`, nothing filed | `Err` — a terminal end-of-input, still nothing filed |
   | `repeated().delimited()` over `"( 1 2 3 )"`, element accepts consuming nothing | `Ok([1, 2, 3, …])`, nothing filed | `Err` — likewise |
   | `separated_by(..).delimited()`, element consumes and declines before the closer | `Ok([1, 2, 3])`, nothing filed | `Err` — likewise |

   **Still unchanged**: the mid-scan closer in `separated_by(..).delimited()`. It is reached from the
   top of a cycle, so only an *accepting* element can precede it — a decline and a stall each break
   into the epilogue — and the cycle's baseline is taken above it, which makes the term a constant
   `false` there. An `Accept` remains untouched everywhere, for the reason item 56 gives.

   The two delimited `*_while` drivers have real-closer exits too, and **item 58 below** brings them
   under the same gate. `parser::many`'s `GATE_CENSUS` gains a per-source count of real-closer exits
   with a region scan requiring each close verdict to reach the gate before it commits, plus a
   two-directional scan of the gate itself — the counter read, the position *not* read, since a
   scanner term smuggled in there is as much a defect as a missing descent one. — *(#148 R8)*

58. **The same trip, through the same absence exits, in the other eight drivers.** Items 56 and 57
   closed this for the four **try-driven** collection families and recorded the other eight
   guard-bearing sources — `repeated_while()`, `separated_by_*_while()`, both of their delimited
   forms, and the fold sources behind `fold`/`try_fold`/`try_fold_with`/`rfold` and their four
   `*_while` twins — as *found, not fixed*. They are fixed now.

   Their exposure was **narrower** than the four's, not different: none of them files an element's
   `Err`, so a trip an element *hands back* propagates untouched and was already terminal there. The
   hole was the exit with no `Err` in it — an element that catches
   [`RecursionLimitReached`](https://docs.rs/tokora/latest/tokora/error/struct.RecursionLimitReached.html)
   itself and then reports *no more elements*. Measured rather than inferred, on the code as it
   stood:

   | driving an element that catches a depth trip and then reports absence | before | now |
   |---|---|---|
   | `fold(..)` over `"1 2 3"`, element declines | `Ok(6)` under a budget the element exceeds — identical to `Ok(6)` under one it does not | `Err` — a terminal end-of-input, still nothing filed |
   | `repeated_while(..)` over `"1 2 3"`, element accepts consuming nothing | `Ok([1, 2, 3, -1])` under both budgets | `Err` — likewise |

   Closing it needed a per-**element** trip baseline none of those loops took, which is a shape
   change rather than a gate addition: three of the folds were `while let Accept(..) = element(..)`
   loops, and a `while let`'s condition *is* the element attempt, so there is nowhere in one to
   snapshot the counter before it. Those three are now `loop { let trips = …; match … }`. The
   remaining five take the baseline at the top of the cycle, which is once per element, since a
   cycle attempts at most one.

   Thirty absence exits across all twelve sources now route through `absence_after_element`, and
   eight real-closer exits through `close_after_element` — every `CloseStatus::Close` verdict in the
   tree, with **no per-site exemption**. Where a probe sits at the top of a cycle its descent term is
   a constant `false`, and the call is kept anyway: a `usize` comparison costs less than an
   exemption table, it fails closed if a later refactor moves an element attempt above the probe,
   and it is what lets the census scan each verdict's own arm instead of comparing tallies. The two
   **direct** closers — `separated_by_*(..).delimited()`'s and `separated_by_*_while(..).delimited()`'s
   mid-scan arms, which commit from the driver's own scan with no probe verdict — stay exempt for
   the structural reason item 57 gives, and are counted as such.

   **Unchanged, and deliberately so**: an `Accept` is still never gated, in any of the twelve, for
   the reason item 56 states. So is each `*_while` driver's `Action::Stop` exit in practice — it is
   reached before that cycle's element runs, so the element that could have caught a trip is the
   previous cycle's *accepting* one, and the term there is a constant `false` by construction rather
   than by exemption.

   `parser::many`'s `GATE_CENSUS` loses its two-shape classification: `absence_exit_shapes` has no
   zeroes left, every source is required to spell **neither** witness itself, and a new
   `every_driver_baselines_its_trip_witness_inside_its_element_loop` pins one `trip_snapshot()` per
   element loop in all twelve — matched against the loop openers as a second, independent needle —
   and requires every one of the forty-two chokepoint calls to read a baseline taken inside the loop
   that hosts it. — *(#148 R9)*


59. **A trivia skip whose predicate panics no longer loses the token it was asked about.** On a
   [`Complete`](https://docs.rs/tokora/latest/tokora/input/struct.Complete.html) input,
   [`skip_while`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.skip_while)
   reaches its lexer once the stream is drained, and the token it lexes was handed to the
   predicate while nothing owned it. A predicate that unwound therefore dropped that token: the
   call resumed from the previously committed span and the next read **re-lexed** it, where the
   same skip on a sealed
   [`Partial`](https://docs.rs/tokora/latest/tokora/input/struct.Partial.html) input — whose scan
   scope holds the token for exactly this reason — put it back at the front of the stream and
   resumed there. Two typestates, one primitive, two different resume cursors under
   `catch_unwind`.

   The token is now owned by a guard across that one call, so an unwinding predicate leaves it
   parked rather than dropped, and the ordinary stop and the unwind edge perform the identical
   put-back. **For an unwind out of the predicate** — at every call, over every residency and cache
   capacity swept — both routes now leave the same cursor, the same front residency and the same
   amount of re-lexing. They still part company on one other exit, an unwind inside the
   end-of-input settle, which was left as it is on purpose; entry 54 below says why and names the
   cell that pins it.

   Visible only to a host that catches an unwind out of its own `skip_while` predicate and keeps
   using the input; a predicate that returns normally, or a panic that aborts, is unaffected.

   **It gives back about three fifths of entry 54's win — the two fifths that remain are what
   ships — and that is stated rather than buried.** Measured on the same harness and in the same
   interleaved runs as the figure there: 1321.0 µs → 1482.5 µs on the 57 kB document and 133.7 µs →
   148.1 µs on the 7.5 kB one, so **+12.2% and +10.8%** of a whole parse. Entry 54's route was 17.4%
   and 15.9% faster than the previous release line before this guard and is 7.4% and 6.9% faster
   with it, so the guard hands back 10.0 of those 17.4 points and 9.0 of those 15.9 — 57.5% and
   56.6%.

   The whole of the cost is the **unwind cleanup region** a destructor opens in the lexing loop,
   not the guard's own work: the identical wrapper with its `Drop` deleted is free, and so is the
   guard itself compiled `panic = "abort"`. Six shapes were built and timed and this is the
   cheapest — scoping the guard tighter, taking its `Drop` out of line, moving the predicate call
   into a non-inlined helper, and outlining the whole lexing phase are all worse or no better. The
   resident-token path — the 22,033-of-39,057 skips that skip nothing, and every skip over a warm
   cache — builds no guard and is unaffected.

   It was judged correct to pay because a panicking predicate is **contract-covered behaviour**,
   not undefined territory: `skip_while` documents a *Panic unwind* section promising that no token
   leaves the stream, and the predicate is explicitly outside the "inert callbacks" precondition
   that scopes the rest of the fast path's guarantees. Two typestates disagreeing there falsifies a
   documented promise, and entry 54's own claim rests on panic tests as its evidence.

   **That same reasoning is why the other divergence was not paid for.** The end-of-input settle
   exclusion in entry 54 is reachable only through the lexer, the source, the span or the offset —
   all of them *inside* the inert-callbacks precondition, so no caller who meets the condition can
   provoke it — and the complete-input route's answer there is the stronger of the two, not a
   defect being tolerated. Where a promise was over-broad the promise was narrowed and the
   behaviour pinned; where the behaviour was wrong, as it was for the predicate, it was fixed.

   The differential sweep that should have caught this could not: its check on where the stream
   resumes was a *range* between the committed position and the next token's start, and a token
   put back and a token dropped are both inside it. It is an equality now, and a new cell compares
   the two typestate routes field by field after a panicking predicate — cursor, front residency,
   whether the token was re-lexed, and the committed-token notifications the interrupted call had
   already made. That last is also a new observer for a gap noted and left open in the previous
   round: `Emitter::commit_token` reaches no value a caller reads back off the input, so a skip
   that consumed a **parked** token and told nobody had, until now, nothing watching it.

60. **The same wrong-machine wall-clock bound was still sitting in `pratt_limit_unit_sink`, and it
   took the `miri-tb-x86_64-unknown-linux-gnu` leg red on `main` — 57 minutes in.** The entry
   further up this section corrected `pratt_limit`'s deep-stack bound and did not sweep for
   siblings. There was one: `bounded`, the wall every cell in `pratt_limit_unit_sink` runs under,
   with 60 seconds hard-coded at all thirty-one call sites. On a compiled build 60 seconds is
   enormous — this file's slowest cell, the section 3 unwind cell, runs in 9.8–10.4 ms and no
   other cell reaches a millisecond — and under interpretation it is a coin flip. Measured with
   the exact command and `-tb` flags `ci/miri_tb.sh` runs, timing the interval `bounded` itself
   bounds:

   | | native | Miri, aarch64-apple-darwin | slowdown |
   |---|---|---|---|
   | the unwind cell, alone | 9.8 ms | 49.72s / 48.61s | about ×4,800 |
   | the unwind cell, in a whole-file run | 10.4 ms | 49.32s | about ×4,800 |
   | all sixteen cells | 10 ms | 58.02s | — |

   So the cell is essentially the whole file, which is why CI reported a single failing cell
   rather than general slowness. The bound is now 500 seconds under Miri and 60 natively,
   unchanged. 500 is ×10 over the slowest reading, the same multiple the deep-stack bound chose,
   and this time the cross-host gap has a measured floor rather than an argument: the same cell
   that reads 49.7s here tripped a 60s wall on the shared `x86_64-unknown-linux-gnu` runner, so
   that runner is at least ×1.21 slower on an interpreted workload of this shape. A tripped
   deadline reports "over the budget" and never by how much, so ×1.21 is a floor; ×10 leaves room
   over it and over a pessimistic ×3 draw on a noisy runner alike. It is deliberately not larger,
   because a budget is also a bill the job pays if a termination regression ever lands: sixteen
   cells at 500 seconds is 2h13, inside the job timeout, where an hour a cell would be a way of
   never finding out.

   **The two bounds are now one bound.** Both suites' helpers were the same function modulo a
   stack size and a number, and keeping them separate is what let the first fix miss the second
   site. They now both call `common::bounded_wait`, whose allowance is a `WallClock` — a pair of
   figures, one per build kind, both required. A third suite that needs a wall cannot spell one
   without saying what an interpreted build is allowed, which is the property that was missing,
   and `bounded_wait` refuses a pair whose interpreted figure is below its native one, because
   interpretation is not the faster of the two and such a pair is transposed rather than measured.
   Each call site keeps its own measured number and its own reasoning; only the mechanism is
   shared.

   The deep-stack bound's 700 seconds was re-read under Miri with the refactor in place and is
   unchanged — its own doc records the cross-check. No library behaviour changes, and the native
   path is untouched: every cell here finishes in single-digit milliseconds, nowhere near either
   figure. — *(#148, verification debt)*

61. **A scanner resource trip is now terminal for every emitter, not only for the ones that accept
   its diagnostic — recovery re-raises it instead of retrying it.** Item 48 answered the *descent*
   side of "terminality is a property of the event, and this crate enumerates carriers of it". This
   is the *scanner* side of the same root cause, and it was reachable through every fail-fast
   emitter.

   One trip, one position, one committed leaf, two emitters, two verdicts. An **accepting** emitter
   files the trip's diagnostic and the committed leaf
   ([`next_or_stop`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.next_or_stop),
   [`try_expect_or_stop`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.try_expect_or_stop))
   builds an
   [`UnexpectedEot`](https://docs.rs/tokora/latest/tokora/error/struct.UnexpectedEot.html) through
   `into_terminal`, so `is_terminal()` is `true` and recovery re-raises. A **rejecting** emitter
   reports the same trip by returning `Err` from
   [`emit_lexer_error`](https://docs.rs/tokora/latest/tokora/emitter/trait.Emitter.html#method.emit_lexer_error) —
   which is not a refusal to report, it *is* the report — and the scanner propagates that value
   straight out of its trip arm. It was built by `FromEmitterError::from_lexer_error` from the
   lexer's own `Token::Error`; no `UnexpectedEnd` exists on that path, so nothing calls
   `into_terminal` and `is_terminal()` is `false`. Recovery then **ran**: measured through
   [`recover`](https://docs.rs/tokora/latest/tokora/trait.ParseInput.html#method.recover) under
   [`Fatal`](https://docs.rs/tokora/latest/tokora/emitter/struct.Fatal.html), the recoverer
   re-entered the scanner and re-tripped the same limit — scan count 3 → 4 — and returned `Ok`,
   turning a spent resource budget into a successful parse. All four recovery attempts were
   affected: `recover`,
   [`inplace_recover`](https://docs.rs/tokora/latest/tokora/trait.ParseInput.html#method.inplace_recover),
   and both of
   [`skip_then_retry`](https://docs.rs/tokora/latest/tokora/trait.ParseInput.html#method.skip_then_retry)'s
   attempts.

   **The witness already existed and those four sites were the only guarded ones not reading it.**
   The input's poison boundary is the scanner's own record of the trip, and `parser::many`'s twelve
   collection drivers consult it at every absence exit through `latch_snapshot` /
   `latched_during_attempt`. The four recovery sites consulted it zero times. They now read it,
   beside `MaybeTerminal::is_terminal` and the session trip counter — five witnesses, in the five
   places the two never-recoverable conditions are stored.

   **The scanner's witness is a monotone COUNTER, not the boundary that trip latched, and that is
   the whole of the fix.** The boundary is a *lineage memo*: a `Checkpoint` carries it and a restore
   copies it back verbatim. Any comparison of it across a rollback therefore reads a restored value
   against what it was restored to, and moving the comparison closes exactly one level at a time:

   - read **after** the attempt, it compares the latch `try_attempt`'s `Err` arm just restored
     against the value it restored it to — always equal, always `false`. Measured: with the read
     there, only the non-backtracking `inplace_recover` cell passed and all three speculating ones
     still failed with the scanner re-entered after the trip.
   - read **inside** the attempt, that level is closed and the level below opens. Grammar code may
     catch a scanner stop inside an *inner* `attempt` of its own and decline it; the inner rollback
     restores the boundary before the outer gate ever looks, and the outer verdict reads clean over
     a stop that is live, already diagnosed, and re-trips on the next scan of the same prefix.
     Measured the same way: the recoverer ran and the scan count moved 3 → 4.

   Relocating the read a third time closes level two and opens level three. **A cell inside the
   rollback set cannot witness an event across a rollback at any depth**, so the witness is a cell
   no rollback reaches: a new `Input::scanner_trips`, monotone, never lowered, not carried by a
   `Checkpoint` and not by the sync family's `ThroughEntry` either. It is the exact twin of the
   descent counter item 48 added, for the other budget, and it is classified in the same place —
   `input::lineage`'s CELL_CENSUS destructures `Input` exhaustively, so the field could not be added
   without a row in the taxonomy table saying what a restore does to it. Its **one writer** is
   `latch_if_limit_tripped`, the crate's sole terminal predicate: `classify` is its only caller and
   both lexing drivers reach `classify`, so *every* scanner trip is counted, a lookahead's included
   — and a lookahead that trips latches the boundary and still returns `Ok` with a short window,
   which is precisely the trip a driver-level count in `scan_with`'s `Verdict::Trip` arm would have
   missed. The bump runs before the diagnostic is offered to the emitter, so a rejecting emitter's
   `Err` cannot carry the stop out past an uncounted trip.

   The latch reading is **kept** beside it, and demoted rather than deleted: it answers "a different
   stop is standing at the end of this attempt" where the counter answers "a stop happened inside
   it", and every transition this crate can produce that moves the latch also moves the counter. It
   costs one `Option<L::Offset>` clone on a path that already clones one, and removing a witness on
   a subsumption argument is removing it on an argument rather than on a measurement. The witnesses
   are still read **inside** the attempt, for that narrow term's sake, and the verdict rides out
   beside the error.

   **The guard is now one chokepoint rather than four copies, and the false justification that let
   the copies drift is gone.** A new crate-internal `parser::recovery_gate` owns the whole judgement
   — every per-attempt baseline and all five witnesses — and the combinators hand it their attempt
   and match on a three-way outcome. Nothing about the law is spelled at a call site, which also
   deletes a hoistable baseline: `skip_then_retry`'s retry-cycle `trip_snapshot()` had to stay
   inside the retry loop (hoisted, the monotone session counter would refuse every retry after the
   parse's first trip) and nothing checked that it did. There is no longer a baseline there to
   hoist. The comment above the old `Recover` guard claimed the error's own answer "covers a
   *scanner* stop, which the grammar's error type carries" — the first half of which is exactly this
   defect; it is corrected rather than deleted, along with
   [`MaybeTerminal`](https://docs.rs/tokora/latest/tokora/error/trait.MaybeTerminal.html)'s "the arm
   is yours to answer for" clause, which is now true of a
   [`PartialSession`](https://docs.rs/tokora/latest/tokora/input/struct.PartialSession.html)'s
   terminal latch and no longer of the three recovery combinators.

   **`skip_then_retry`'s recovery was not only its retries, and the rest of it ran outside the gate
   entirely.** A gate that wraps the parse attempts constrains what happens *inside* one and says
   nothing about recovery work that never enters one — and this combinator's actual recovering is
   the **skip** to a sync point and the **advance** over a sync point that did not admit a parse.
   Both are `Ok`-returning primitives that fold a terminal trip into the value they use for genuine
   exhaustion: `sync_balanced` answers `Ok(None)` whether it found no sync point or the scanner
   tripped mid-skip, and `next` answers `Ok(None)` whether the input ended or the scan tripped. So a
   spent scanner budget read as *"nothing left to skip to"* and the combinator surfaced its ordinary
   trigger error for it — an error whose `is_terminal()` is `false`, which a `PartialSession`'s
   terminal latch reads and nothing else corrects. Through a **rejecting** emitter this never showed,
   because the rejection propagates as an `Err` out of the skip itself; through an **accepting** one
   the diagnostic is filed and the `Ok` comes back looking exactly like end of input.

   Both phases now go through `recovery_gate::recovery_step`, which samples the three input-side
   witnesses across the operation — it reads three rather than five because the other two
   interrogate an error value and a step that returned `Ok` has none — and a stop inside either
   surfaces the terminal-marked `UnexpectedEot` every other committed exit in this crate surfaces
   for a trip an accepting emitter took. *"The scanner stopped"* and *"there was nowhere to sync
   to"* are two different answers again, and only the second is recoverable.

   **Unchanged:** an ordinary failure still recovers, in all four attempts, and a genuine sync
   exhaustion still surfaces the trigger error — the EOF reading is narrowed, not replaced, and a
   cell pins each half of that pair. The verdict is attempt-relative against per-attempt baselines,
   so a stop an *enclosing* lookahead caused before the attempt started is never charged to it.

   **One source-breaking addition, and it is the only public-surface change.**
   `ParseInput for SkipThenRetry` grows
   `Error: From<UnexpectedEot<L::Offset, Lang>>`, because surfacing a terminal stop means
   *constructing* one and that is the carrier this crate constructs. It is the same conversion
   `next_or_stop`, `try_expect_or_stop`, the peek family and both pratt engines already require, and
   one fifth of `FromTokenErrors`, so a grammar that can reach end of input at all already has it; a
   grammar that cannot gets a compile error naming the missing `From`. There was no bound-free
   alternative — `MaybeTerminal` is a *predicate*, with nothing to build from — and the alternative
   of leaving the stop unmarked is the defect. Everything else is internal: `recovery_gate` is a
   private module, `Input::scanner_trips` is a private field, and its two accessors are
   `pub(crate)`.

   **What it costs, stated exactly:** every witness is read on the failure path only, so a
   successful attempt pays just its baselines — one `Option<L::Offset>` clone plus two `usize` loads
   per attempt. For the two speculating combinators the clone is the same value the checkpoint they
   save clones anyway; `inplace_recover`, which saves none, pays it new. The scanner counter's write
   side is one `wrapping_add` on the trip arm of the terminal predicate, which is a path that has
   already decided to stop.

   **Narrowed on purpose, and this is the one behaviour change beyond the headline:** a scanner stop
   latched *anywhere inside* the attempt now re-raises, including when the attempt then fails for an
   ordinary reason short of the boundary — a wide lookahead that trips, followed by a syntax error.
   That is the same rule `parser::many`'s absence exits already apply, and for the same reason: the
   attempt's evidence was truncated by a stop, and recovering from it would re-lex the same prefix
   and re-trip. It fails closed, and the whole existing suite is unaffected by it.

   **What holds it, and what watched it fail.** A new behavioural suite,
   `tokora/tests/recovery_terminal_stop.rs`, 13 cells. The trip cells are self-calibrating, so no
   absolute scan tally has to be maintained: the primary parser records the scan count at the
   instant its own scan tripped, and each cell requires the counter not to have moved since. Five
   direct trip cells (all four attempts, plus the accepting-emitter control that proves the two
   sinks now agree); two **nested-speculation** cells, in which the primary catches the trip inside
   an inner `attempt` of its own, declines it, and then fails ordinarily — driven through both
   `recover` and `inplace_recover`, because the two rollback postures are what would otherwise make
   the defect look like a property of the *outer* attempt when the *inner* rollback is what defeats
   the latch; two **phase** cells for a trip inside the skip and inside the advance; and four
   scoping cells that go red if the gate starts refusing ordinary failures, one of them the genuine
   sync exhaustion paired against the skip-phase trip.

   `RECOVERY_GATE_CENSUS` covers the structure the suite cannot, and it was extended for the reason
   its own shape made it miss the sync phase: it constrained what happens *inside* the chokepoint,
   which is silent about recovery work that never enters one. It now names the **phases**, not just
   the attempts — every step routes through `recovery_step`, each matches both outcomes, and no
   combinator source reaches a recovery primitive on its own handle (`inp.sync_balanced(`,
   `inp.next(` and three siblings pinned absent, which works off the convention that a combinator's
   handle is `inp` and the chokepoint's closure parameter is `input`). The witness scan is now split
   by subject: the two error-carried witnesses appear exactly once, in `judge`; the three
   input-side ones exactly twice, once in each judging body, each ahead of the verdict that body
   carries out.

   Watched failing three times, each with the whole behaviour suite green: with the latch swapped
   for the positional `at_committed_boundary` (a real regression a rollback over the latch defeats);
   with `Recover`'s guard re-spelled by hand, correctly; and — the extension's own demonstration —
   with the scanner counter deleted from `recovery_step`, where the behaviour suite stays 13-for-13
   because the latch reading beside it answers the same way on every case anyone wrote, and the
   census reds on the count. That is the shape the census exists for: a witness redundant on the
   cases in the suite and load-bearing on the ones that are not.

   Two frontiers are stated on it rather than left to be found. It reads two combinator sources plus
   the chokepoint, so a recovery combinator added in a *new file* is not read until it is listed;
   and the ungated-primitive check is a **list**, so a recovery phase built on a primitive nobody
   added is a phase nothing reads.

62. **The collection drivers had the same rollback hole item 61 closed for recovery, and the
   monotone counter it added now closes it here too.** Item 61 said, correctly, that `parser::many`'s
   twelve collection drivers "consult the poison boundary at every absence exit". What it did not
   say is that those drivers were reading it with exactly the defect it was in the middle of fixing
   one module over: the boundary is a lineage memo, and a driver's read of it sits outside every
   rollback the elements below it may perform.

   An element is entitled to open an [`attempt`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.attempt)
   of its **own**, meet a scanner resource trip inside it, catch the stop, and decline that attempt.
   The inner restore then rewinds the cursor, the cache, the emissions **and** the poison boundary
   together — back to the value the *driver* snapshotted before its element loop. Every witness the
   drivers had answered `false` afterwards, over a stop that is live, already diagnosed and re-trips
   on the next scan of the same prefix:

   - **34 gates** read clean. The 30 absence exits concluded "no more elements" and returned a
     silently truncated `Ok`; measured, `peek`-free `repeated()` over such an element returns
     `Ok([])` on `"1 2"` under a budget the element spent, indistinguishable from the same parse
     under a budget it did not.
   - the **4** `file_element_failure` swallow sites were worse than a missed re-raise. There the
     element's ordinary `Err` was **filed as a recoverable diagnostic and the loop carried on** — so
     the run also gained a syntax error describing input the scanner never read, and the rollback
     had already erased the trip's own diagnostic, leaving a log that does not mention the stop at
     all.
   - the **8** real-closer gates are unaffected and stay that way, deliberately. A committed
     pre-trip closer settles the scanner question, and the lookahead that latched past that closer
     is the same one that bumped the counter — so a counter reading there would refuse exactly the
     delimited parses the latch reading would, all of which a wider scan window completes to the
     identical value.

   Neither companion witness could have covered it. `at_committed_boundary()` is positional and the
   restore rewinds the cursor too; `tripped_during_attempt` counts *descent* trips, and a scanner
   trip bumps no descent counter.

   The fix is item 61's, reused rather than reinvented: `Input::scanner_trips`, the monotone session
   counter whose sole writer is the crate's terminal predicate, read through
   `scanner_trip_snapshot` / `scanner_tripped_during_attempt` at
   `absence_after_element` and `file_element_failure`. Nothing `pub` changed and no bound moved.

   **Its baseline is per COLLECTION, and that is the opposite of the descent counter's.** The two
   facts decay differently: a descent trip an element caught and legitimately parsed past stops
   being true of the input, so its baseline is re-taken per element; a spent scanner budget does
   not, so its baseline is taken once, above the element loop, beside `latch_snapshot()` whose
   question it answers. Per element it would drop the case *element 1 trips and accepts, element 2
   declines* — element 2's baseline would be taken after element 1's trip and the exit would read
   clean — which is a case the latch beside it has always refused.

   **The latch reading is now fully subsumed, and is kept anyway.** Every transition this crate can
   produce that moves the boundary goes through `latch_if_limit_tripped`, which bumps the counter
   first. Measured rather than argued: with `latched_during_attempt` neutered in place and the
   counter left in the condition, every test binary of `--all-features` passes — including the
   `absence_terminal_stop.rs` cells the latch used to be the sole holder of. It stays for the reason
   the recovery gate's equivalent term stays: it is the reading the boundary itself is the subject
   of, it costs one clone already being paid, and a witness removed on a subsumption argument is a
   witness removed on an argument rather than on a measurement. `file_element_failure`'s positional
   `at_committed_boundary()` is **not** subsumed and is load-bearing — it sees a boundary latched
   before the driver started that the element has now run onto, which no per-collection counter
   baseline can see.

   `GATE_CENSUS` grew the witness and a placement test that is the exact mirror of the descent one:
   the scanner baseline must be taken once per collection and **outside** the element loop, where
   hoisting the descent baseline would itself be the defect. Watched failing with the whole
   behaviour suite green — the extension's own demonstration: with one driver's baseline moved
   inside its loop, `every_driver_baselines_its_scanner_witness_once_per_collection` is the **only**
   failing test in all 105 binaries, because no behaviour cell anyone wrote builds the shape that
   placement drops. Two new regressions pin the behaviour itself, one per gate, each watched failing
   first and each with a widened-limit control that differs in nothing but the scan budget.

63. **A peek that reached the lexer reserved two token windows, not one.** The fill allocated the
   public `Peeked<W>` deque the caller gets back, and then — on the cache-miss path — a second
   `W::CAPACITY`-slot store of the same entry type, a `GenericArray<MaybeUninit<_>, W::CAPACITY>`
   holding the tokens lexed past the cache until the cache region had been copied out. The window
   bound caps the slot *count*, but `Token`, `State` and `Span` are unconstrained in size, so a
   grammar with a large token payload or a large lexer state paid **twice** the window it asked
   for on every peek that reached the lexer. For the crate's own oversized fixture — a 1 KiB token
   beside a 1 KiB lexer state, 2,072 bytes an entry — that is 132,632 bytes at `U32` and 33,176 at
   `U8`; those are now **66,320** and **16,592**. Nothing about the peek's result changes; this is
   stack the caller was paying for and could not see. It matters most where the stack is smallest,
   which is the `no_std` target the crate advertises.

   The second store existed to solve an **ordering** problem, not a storage one. A window's two
   halves are produced in the opposite order from the one it must report: the tokens past the
   cache are lexed *during* the fill, while the cache region is copied out in one bulk read
   *after* it, and `Cache::peek` only appends. That needs the staged region to end up *behind* the
   cache region — not a second array. So it is staged at the tail of the window the caller already
   owns and rotated back once the cache region is in. The rotation permutes values inside the
   deque and copies none out, and the deque owns each staged entry from the moment it is pushed,
   so every exit — success, a withheld frontier, a limit trip, a fatal emit — frees it exactly
   once. `input/input_ref/` now contains no `unsafe` block at all; the two it had were the removed
   array's partial-initialization bookkeeping.

   Reordering the copy *ahead* of the loop would need no rotation and no check, and it is not
   available: `Cache::peek` takes `&'p self` and the entries it appends borrow the cache for the
   whole of the fill, while the loop's every step — `lex_within_boundary`, `classify`,
   `emit_lexer_error_deduped`, `cache_append` — takes `&mut self`, and `cache_append` mutates the
   very cache those entries borrow.

   **The reordering creates one hazard, and it is checked.** `Cache::len` is now load-bearing: the
   fill reserves the window's cache region from it *before* it stages anything, so a `len` that is
   not the resident count mis-sizes the copy that follows. An under-report clips the copy mid-run
   and the rotation then closes the gap with staged tokens that belong *after* the residents the
   clip dropped — not a short window, a **hole**, at a position the next consume will not serve.
   `Cache::len` and `Cache::peek` now say so where an implementor reads them, and the fill exit
   that rotates checks the copy it got, in **every build**: a count identity, and — because an
   inexact `len` can satisfy any count derived from it — the resident run's own `front`/`back`
   endpoints, gated on the staged count, which is a number the fill computes rather than one the
   cache reports. The endpoint witness is the release-critical half, not the optional one: it is
   the only check that sees a clip landing exactly on the window's edge, or a `len` under-reporting
   all the way to **zero**, which passes the count trivially as `0 == 0`. Both of those are the
   hole. `InputRef::peek` grows a `# Panics` section. The two exits that answer from tokens already
   at the front of the stream append nothing behind their copy, so they cannot hole, and they are
   unchanged and unchecked.

   **The hot read pays nothing for it, and that took measuring.** The half of the fill that
   reaches the lexer is now `#[inline(never)]`: staging in the window puts a deque push, a
   truncate, a rotation and a `Cache::peek` whose room is a runtime value into the same body as
   the resident-head arm, and the register allocation and block layout that follow are shared.
   Left in one function, six peek-shaped bench ids paid **1.11×–1.31×** for code they never run —
   and the checks are not what they were paying: with both removed entirely the same six ids
   still read 1.11×–1.31×. Split out, with the overflow-free fill taking its own exit and the
   panic message built out of line, the same six read **0.80×–1.15×**: the two `heavy` ids — a
   large lexer state, exactly the shape the second window cost the most — come back **20%
   faster**, and the worst reading is `input/scan/peek1_then_next` at 1.15×, a drain that defeats
   its own cache by construction so that every read is a fill. Geometric mean across the six,
   0.98×. Nothing outside the peek family moves.

   The same law decided where the release-active endpoint witness lives, and it cost one more
   measurement to find out. Inlined into the fill it sits past the overflow-free `return`,
   unreachable from the arm that takes it — and moved `peek1_then_next` by **1.025×** anyway,
   consistently and outside the noise, for exactly the reason the split above exists. So
   `assert_cache_copy` is `#[inline(never)]` too, and its second message joins the first in a
   `#[cold]` free function. Out of line, the six ids read **0.9987** geometric mean against the
   window fix alone, every one of them inside 0.974×–1.009×, over ten interleaved criterion
   rounds. The 67–70-instruction figure that once argued for keeping the witness in debug builds
   is withdrawn: it was the cost of running the check, and what a hot path actually pays is the
   cost of *stepping over* it.

   Coverage, since the previous overflow tests asserted only `len()`: seven cells pinning window
   order across the cache/overflow boundary — the smallest overflow, a prefilled cache, a cache
   that retains nothing, a parked front token, and the widest window over an oversized token and
   state — which removing the rotation turns red along with the pre-existing
   `token_accessor_reads_owned_arm`; a drop-counted cell for the *keeping* exit; a second phase on
   the fatal-emit cell that reads the buffer back after the failing return; compile-time size
   assertions over the oversized fixture; and a source census that refuses to let the fill body
   name an array, a deque, a `MaybeUninit` or `unsafe` again. The two `should_panic` cells that
   exercise the endpoint witness alone — the under-report by one, and the under-report to zero —
   are ungated and run under `cargo test --release` as well, which is the build the witness was
   promoted for; a `should_panic` whose panic compiles out passes by never running the code it
   names. The drop-safety cells now count into a per-scenario `Rc<Ledger>` instead of `static
   AtomicUsize`: they assert *live* deltas while their own window is still held, so under Rust's
   default parallel test runner they were counting whatever else happened to be running —
   `cargo test overflow_peek` failed 184 runs out of 200. —
   *(#156)*

64. **The peek fill had two paths no benchmark in this repository entered — including the one
   item 63's rotation and its release-active endpoint witness live on — and one bench file kept
   its evidence where nothing could check it.** No library behaviour changes; both halves are
   benchmark-suite defects.

   `InputRef`'s peek fill returns early when the window is already met (`want == 0`, no lexer
   touched) and, on a window wider than the cache, stages the surplus tokens at the tail of the
   caller's own buffer and rotates them in behind the cache region. **Neither path was reachable
   from a single shipped bench id.** Every peek id read width 1 and consumed what it peeked
   before peeking again, so `want` was always 1; and `DefaultCache` is three slots, so nothing
   narrower than a width-4 peek can overflow. That was measured, not argued: with the sites
   instrumented to panic, all 45 ids across `input_scan`, `parser_combinators`, `pratt_typed`,
   `cst` and `backtrack` ran clean. Item 63 disclosed the same gap from the other side — it
   promoted a `Cache`-contract endpoint witness to release-active precisely because a clipped
   copy rotated in behind staged tokens holes the window rather than shortening it, and no
   shipped id reached the rotation that check guards.

   A new `input/peek` group in `tokora/benches/input_scan.rs` adds `r4_peek1_hit8` (a width-3
   peek primes the cache, three width-1 peeks then take the hit exit) and
   `r4_peek8_heavy_staged` (a width-8 peek over the three-slot cache, under the deliberately
   heavy lexer state, so five tokens overflow and the `L::State` clone they pay is a visible
   256-byte copy). Under the same instrumentation `r4_peek8_heavy_staged` trips all three sites
   of the miss path — the staging push, `assert_cache_copy`'s endpoint half, and the
   `rotate_left` itself — `r4_peek1_hit8` trips only the hit exit, and no pre-existing id trips
   any of them. The arithmetic that reaches each exit is also a `const`-gated assertion inside
   the driver itself, run once outside the timing loop, so a probe that stopped reaching its path
   would fail the bench instead of reading as coverage; the existing groups' ids, fixtures and
   baselines are untouched.

   `tokora/benches/pratt_typed.rs` had 758 comment lines carrying measured figures and structural
   claims that nothing could verify — seven review rounds had produced about twenty corrections to
   its prose and none to its code. The structural claims are now assertions that run before either
   entry point registers anything: fixture byte-density counted from the generated bytes rather
   than from the generator's loop, the right/left pair proved equal except at exactly the operator
   positions, the checksum's depth-1 associativity blind spot pinned in both directions, and the
   recursion-budget rationale executed against the deepest fixture instead of argued. Every
   remaining figure moved into one dated, attributed record block that states it is not re-derived
   by anything. That pass found the file already carrying a stale claim — it attributed the parse
   recursion default to `RecursionLimiter::new`, which item 50 of this release returned to 500;
   the 64 in question comes from `ParserContext` and the input layer — plus two source line
   numbers into another file, now symbol names.

65. **Three public claims corrected where they said more than the crate does. Documentation only —
   no behaviour changed, nothing `pub` moved, no test result changed.** Each was already
   contradicted, or already left unstated, by something measured in the tree.

   **The sync family's panic-unwind claim was false at two of its three sites, and one paragraph
   held all three.**
   [`sync_to`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.sync_to),
   [`sync_through`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.sync_through)
   and
   [`sync_balanced`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.sync_balanced)
   carried the same sentence byte for byte: a panic out of the predicate, the expected-tokens
   closure, the lexer or the emitter "settles — this method's `to`-shaped commit posture keeps the
   diagnosed prefix; the rewinding scans (`sync_through`, `sync_balanced`) restore to the call's
   entry instead". The clause names its own two counterexamples in its own next breath. `sync_to`
   commits; `sync_through` **consumes** on a stop, committing at the matching token's own span, and
   **rewinds** at a no-match end of input; `sync_balanced` crosses the axes — it stops before the
   sync token and keeps the skipped prefix like the first, and rewinds at end of input like the
   second, which is why its unwind disposition belongs to the *exit* rather than to the mode. Three
   postures cannot share one true sentence. Each method now states its own, with its own
   measurement: `r9_stop_exit_panic_still_commits_the_diagnosed_prefix` for `sync_to`;
   `sync_through_unwind_restores_emissions` and `sync_through_warm_unwind_prices_its_re_lex` for
   `sync_through`; and the pair
   `r9_balanced_stop_exit_panic_keeps_the_prefix_like_its_own_stop_does` /
   `r9_frontier_commit_interrupted_abandons_rather_than_half_keeping` for `sync_balanced`.
   `sync_balanced`'s paragraph also stops calling its prefix *diagnosed*: that scan makes no
   per-token report at all. The **in-flight token** is stated as the two-case fact it is rather
   than a blanket put-back: the scanner's `TokenSlot` separates a token still *held* — true across
   the predicate and nowhere else, where an unwind returns it to the front of the stream — from one
   already *handed over* to the stop settle, where the put-back is precisely the step the panic
   interrupted and so cannot happen. There the retained stream is cleared instead and the region
   re-lexes from the committed position, which reproduces the token; what is lost is the cache's
   memo of it, not a token. `r9_stop_exit_panic_still_commits_the_diagnosed_prefix` measures that
   handed-over case, and the two rewinding scans reach their keeping arm only on the same side of
   the split. No behaviour changed anywhere; only the sentences were wrong.

   **One clause is genuinely common, and it is the one that was newly needed.** The exclusion
   [`skip_while`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.skip_while)'s
   claim was narrowed for — *anywhere but the end-of-input settle* — is a property of the shared
   scan scope, which every mode's end-of-input exit disarms *before* its settle runs, so all four
   carry it. For a committing scan that is where the whole diagnosed prefix goes: the skipped run
   sits in an uncommitted frontier, the unwind drops it, and the position stays at the call's entry
   with the diagnostics already emitted standing over a prefix it no longer covers —
   `eof_commit_interrupted(true)` reads `((0, 0), 0)`, the entry position beside the entry state.
   For the two rewinding scans it costs nothing, because there that settle *is* the restore and the
   scan committed no progress for a dropped frontier to strand; what is at stake is the rest of the
   restore, swept at every `L::Offset` clone by `r9_restore_entry_is_atomic_at_every_offset_clone`.

   **The byte-identity requirement that produced this is retired, and replaced rather than
   dropped.** Requiring the three paragraphs to hash identically was doing real anti-drift work, and
   it could only ever be satisfied by making two of the three wrong. A `POSTURE_CENSUS` in
   `src/input/input_ref/census_tests.rs` takes the work over in three parts: all four sections must
   carry the shared exclusion; the three sync sections must stay pairwise **distinct**, each saying
   it states *this method's own* posture, with none carrying the retired sentence again; and — the
   part that actually looks at behaviour — each section must state the posture its own `ScanMode`
   **has**, its required *and* forbidden phrases derived per method from `HOLDS_ENTRY`,
   `COMMITS_FRONTIER_ON_STOP` and `REPORT_SKIPPED` as they stand in `scan.rs`, over a pin on the
   `ScanScope::keeps_on_unwind` formula those phrases are keyed to. Shared-clause presence and
   distinctness alone would pass three paragraphs that are pairwise different and wrong about all
   three methods; the derived check fails on any permutation of them, and flipping one of the
   constants fails the doc until the prose follows it. Prose is compared with `///` stripped and
   whitespace collapsed, so rewrapping a paragraph is not drift. Every check was watched failing
   against the defect it names — each section swapped for another's in turn, and all three rotated
   together for the case only the derived check can catch.

   **Why a *consuming* `Accept` is exempt from every terminality witness — and why a zero-width one
   is not — is now said in public.** Items 48, 61 and 62 finished a programme that put three
   witnesses under the never-recoverable law — the descent counter, the scanner counter for the
   recovery combinators, and the same scanner counter for the collection drivers. The docs said
   where each witness sits; the reason the **accept channel** is exempt from all of them was written
   only in `parser::many`'s module docs, whose `//!` text rustdoc does not render because the module
   is private.
   [`MaybeTerminal`](https://docs.rs/tokora/latest/tokora/error/trait.MaybeTerminal.html) now states
   it where the contract itself is described: every gate under that law sits on a channel where
   *this crate* draws the conclusion — a recoverer synthesizing a value, a driver concluding a
   construct ended from a decline, a stall or a closer — and none of them is consulted when the
   grammar produces the value itself **and consumed input to produce it**. So an element that
   catches a trip, still consumes and returns `ParseAttempt::Accept` spends it, permanently and by
   design: gating that would mean a value-producing parser can never recover from a budget it
   deliberately caught, which is a contract this crate makes for no other error.

   **The exemption stops at a zero-width `Accept`, and the section says so outright rather than
   leaving it to be inferred.** A value returned without consuming anything has produced a value but
   not moved the parse, and the driver's very next act is to read that absence of progress as *no
   more elements* — a conclusion of the driver's own, gated through the same chokepoint a decline
   reaches and by all three witnesses at once. That is not a corner case: the four `*_while`
   collection drivers and the `*_while` folds take their element through `ParseInput`, which has no
   decline channel at all, so a zero-width return **is** their decline — which is exactly the
   hole item 58 closed for the descent witness and item 62 for the scanner one. Publishing the
   exemption
   without its boundary would have restated, as a contract, the shape two shipped fixes had just
   removed. The "answered versus swallowed" framing does not settle this case by itself, so the
   boundary is drawn on what the return *consumed* instead.
   [`RecursionLimitReached`](https://docs.rs/tokora/latest/tokora/error/struct.RecursionLimitReached.html),
   which stated the exemption for its own stop only, now points at the general statement and carries
   the same limit on it.

   **`Emitter::rewind`'s "report before you mutate" rider now says what it does not buy.** The rider
   requires a detecting emitter to raise its unpaired-settle report from a preflight over unchanged
   state, so a host that catches it holds the emitter it had rather than a half-rewound one — and it
   stopped there, which invites reading the whole operation as recoverable. It is not:
   [`rewind`](https://docs.rs/tokora/latest/tokora/emitter/trait.Emitter.html#method.rewind) is
   called from the middle of a checkpoint restore, below the point where the lineage has been popped
   through the target and any session points above it released, and above the point where the
   position, the error-reporting witnesses and the cache-push counter are installed. A compliant
   preflight makes the *emitter* transactional and cannot make the *restore* so; a host that catches
   the report must treat the parse as over rather than resume on that handle. The tear itself was
   already measured — `restore_unchecked_is_not_transactional_across_the_settle_wall` — and already
   stated on
   [`InputRef::restore`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.restore),
   but only there, behind the `unstable-raw` feature, where a consumer of the recording CST `Sink`
   never reads it. The bound now sits on the always-compiled trait contract that grants the panic
   door in the first place.

67. **Three corrections to the `Cache` trait's own contract, and the conformance kit made
   falsifiable against two of the three.** A third-party implementor has only the trait docs to
   go on, and two of its load-bearing sentences said something the law stated elsewhere in the
   same doc comment contradicted.

   **`peek`'s bound was corrected from the buffer's total capacity to what it has left.** The
   trait's contract bullet and `peek`'s own `# Parameters` note both said `peek` appends
   `min(len(), buffer capacity)`; `peek`'s `# The law` section, in the same doc comment, already
   said *remaining* capacity. A `peek` written to the wrong half of that contradiction — reading
   `buf.capacity()` where it should read `buf.remaining_capacity()` — computes the same bound as a
   conforming one whenever `buf` arrives empty, and a different one as soon as the caller hands it
   a buffer already holding a parked token or staged overflow, which is the shape `InputRef`'s own
   peek fill produces on the paths that reach this call. Whether that wrong bound is ever *visible*
   is a separate question, and the answer is the uneven coverage described below: an implementation
   that clobbers or reorders what `buf` already holds leaves a trace and is caught, while one that
   simply drops the surplus leaves none. All three summaries now agree: the bound
   is `buf`'s **remaining** capacity *at call time*, and `peek` appends *behind* whatever `buf`
   already holds, never over it.

   **`Cache::len`'s panic clause is qualified to the fill path that reaches the lexer.** It read
   as an unconditional "the fill checks the copy it gets and panics"; the check only ever runs on
   the path that reaches the lexer, matching what `InputRef::peek`'s own `# Panics` already
   scoped it to. The trait doc now says so too, instead of promising a check the cache-hit exit
   never performs.

   **A law that was only implied is now stated outright: a push is refused only when the cache is
   full, on both arms, not just `push_back`'s.** `push_front`'s own doc said "if the cache is
   full, returns `Err`," which reads as one licence among possibly others; it is the only one.
   The trait's contract bullet, `push_front`'s own doc and `RETAINS_FRONT`'s doc all say so now:
   declaring `RETAINS_FRONT = false` buys back the input layer's parked-slot fallback, it is not
   a licence to refuse a `push_front` into an empty, non-full cache.

   The conformance kit is what changed underneath all three, though not evenly, and the uneven
   part is worth stating rather than rounding off. The refusal law it now enforces outright. The
   `peek` bound it enforces only where a violation is *observable*: a `peek` that clobbers or
   misorders what `buf` already holds is caught, at every residency and every prefill depth, but a
   silent total-capacity `peek` — one that computes the wrong bound and drops the surplus without
   trace — is **provably invisible** from outside, because a full `GenericArrayDeque::push_back`
   returns the value and leaves the deque untouched, and `min(min(len, W), R)` equals `min(len, R)`
   for every width and prefill. That one is not rejected; it is *pinned*, by a test asserting the
   kit accepts it, which fails the day the blind spot closes. And `len`'s panic clause the kit
   cannot reach at all, for a structural reason rather than a hard one: the check runs inside
   `InputRef`'s peek fill, and the kit drives a `Cache`'s own methods directly without ever
   building an input to reach that fill through — the panic is perfectly catchable, the kit
   catches twenty of them, what is missing is the call path.
   `tokora::conformance::cache` now
   drives `peek` at every residency and every buffer-prefill depth instead of one fixed shape,
   and drives the empty-cache refusal law at every nonzero capacity regardless of what
   `RETAINS_FRONT` declares — so a `Cache` certified with `CacheHarness` after this change
   carries a materially stronger guarantee than one certified before it. The kit's module docs
   also gained a section naming what it still does not check; the sharpest entry in it belongs to
   the same audience as the three corrections above: every oracle in the kit compares **spans
   only** — never the token or the `L::State` beside it in the same `CachedToken` — so a cache
   that returns the right spans while corrupting either one still passes in full.
   — *(#172)*

69. **`is_empty()` was never asserted false by the cache conformance kit — a fixture returning
   `true` unconditionally passed `CacheHarness::run()` at every capacity the kit drives.**
   `is_empty` is a *default* `Cache` method (`len() == 0`) an implementation is free to override,
   and the kit called it from exactly one place, `assert_empty`, always against a cache `len()`
   had already established was empty. `assert_resident` — already run at every residency every
   other check sweeps, full and partially drained from both ends — now also checks
   `is_empty() == want.is_empty()`, so a constant answer is caught the first time the kit drives
   a non-empty cache instead of never.
   — *(#180)*

70. **`clear()` and `span()` had the same one-shot blind spot in the cache conformance kit, in
   two different shapes.** `check_clear` filled a cache, cleared it, asserted the check-1
   invariants, and never touched it again — so a `clear` that empties the cache and also
   **permanently disables it** (refuses every push afterward, with capacity to spare) passed
   exactly like one that only empties it. It now refills the cleared cache with `cap` fresh
   pushes and checks the refill against check 3's oracle. Separately, `assert_empty`'s own pop
   probe called `pop_front`/`pop_back` on a throwaway `C::new()` rather than on the cache
   `check_clear` had just emptied, so neither call site actually asked the CACHE UNDER TEST
   whether its own pops still answered; `assert_empty` now takes that cache by `&mut` and probes
   it directly.

   `check_span` called `span()` exactly once, immediately after a fresh fill — a residency at
   which `span()` cannot yet be stale, so a `span` that is correct there and never updated after
   a `pop_back` passed unnoticed. It now sweeps every residency `peek`'s check already sweeps:
   full, and partially drained from both the front and the back.
   — *(#180)*

71. **`pop_front_if`/`try_pop_front_if`/`push_many` were never called by the cache conformance
   kit, so a broken override of any of the three passed `CacheHarness::run()` unnoticed.** All
   three are *default* `Cache` methods composed from `front`/`pop_front`/`push_back` — already
   checked exhaustively — so an implementation that does not override them was already correct.
   What was untested was an override: one that removes on a false predicate (or removes and
   reports `None` regardless of what the predicate said), or one that silently discards tokens
   that do not fit instead of handing them back through `push_many`'s overflow iterator. The kit
   now drives both `pop_front_if` and `try_pop_front_if` against a false predicate, an
   `Err`-returning one, and a true/`Ok(())` one, on both an empty and a filled cache, and drives
   `push_many` with two more tokens than capacity, checking the accepted prefix and the refused
   overflow separately.
   — *(#180)*

72. **`peek_one` was never called twice against one unchanged cache by the cache conformance
   kit, so an answer that is correct once and wrong on every repeat passed
   `CacheHarness::run()`.** `peek` has been held to that law since the kit existed; `peek_one`,
   which is composed from it, had not — the kit called it exactly once per residency its sweep
   visits, against a cache instance built fresh for that residency, so a second answer had
   nothing to disagree with. It is now called twice at every one of those residencies.
   Separately, the assertion that `peek_one` names the front entry had **no mutant behind it at
   all**: `peek_one` is a default method, no fixture had ever overridden it, and a fixture that
   does not override it is correct wherever `peek` is. Both halves now have one.
   — *(#180)*

73. **`front()` and `back()` were checked for presence and never for identity outside the empty
   state, so a cache whose specialized span accessors were right and whose entry accessors were
   wrong passed the cache conformance kit.** `assert_resident` — the oracle every residency check
   in the kit runs through — read `front_span()`/`back_span()` and nothing else, and nothing
   required the two pairs to name the same entry. `front_span`/`back_span` are *default* methods
   derived from `front`/`back`, so a cache that gets only `front` wrong was already caught; what
   was not is a cache that specializes the span accessors off a head and tail index, which is
   what a ring does to avoid building a `CachedTokenRef`, and then disagrees. The entry is the
   half that carries the token and the `L::State` a restore resumes from. `assert_resident` now
   reads `front()`/`back()` themselves against the same expectation.
   — *(#180)*

74. **`Cache::with_options` was never called by the cache conformance kit, so a conformant `new()`
   covered for an arbitrarily broken second constructor.** `Cache` has two constructors and the
   kit built every cache it tested with the first, so a `with_options` that came back non-empty,
   or that reported a capacity it would not honour, passed `CacheHarness::run()` undetected. The
   kit could not reach it on its own: `Options` is an associated type it has no way to fabricate
   a value of. `CacheHarness::also_built_by(|| C::with_options(..))` is the new, additive way to
   supply the second constructor; `run()` then drives the **whole** contract a second time
   against caches built by it, re-reading the capacity per pass so the two may differ, and every
   failure message names which constructor built the cache it is about. tokora's own four caches
   are now driven through both. What no kit generic over `C` can check — that the capacity
   matches what the *caller asked for*, since `Options` is opaque — is stated in the module docs'
   "what it deliberately does not check" section rather than left implied.
   — *(#180)*

75. **The cache conformance kit's `push_front` check returned at its first line for capacity 1,
   so the one capacity at which every front push is refused was never driven at all.** The
   prepend *order* genuinely is not observable there — the seeding `push_back` fills the cache,
   so nothing can be prepended — but the refusal is, and nothing else in the kit reaches a
   refused push on that arm: check 3 drives the round-trip for `push_back`, and the two checks
   that drive accepted front pushes never see one refused. A cache that corrupts a refused
   `push_front` — hands back a resident entry and swallows the token it was offered — was
   therefore invisible at capacity 1, which is precisely the capacity where every front push is
   refused. Check 5 now returns only at capacity 0, and asserts at capacity 1 that the refusal
   half was actually reached.
   — *(#180)*

### Performance

The materialization walk was one linear pass **plus a from-zero coverage rescan per gap**,
which is Θ(n²) in the number of diagnosed gaps. As 0.8.0 ships it is a **single** pass that is
linear in events — one walk against a monotone run cursor, with the coverage verdict settled
after the walk rather than before it — **plus one `k log k` ordering of the `k` recorded
diagnostic spans.** With one lexer error per token `k` is Θ(events), so materialization is
**O(n log n)**, not linear; the quadratic term is what this round removes.

This round first replaced the rescan with a *gather* pass in front of the walk, and item 51
below then folded that gather into the walk. Every figure here was originally taken on that
intermediate two-pass shape, so all of it has been **remeasured against what ships** — and one
of them changed sign. Each bullet is the shipped `finish` against the pre-round rescan shape
at `548fd9a`, which is the baseline the superseded figures were against too, with the two-pass
reading kept beside it so the fold's own share stays visible.

A distribution sort that would make it genuinely linear was implemented and **rejected on
measurement**: a fixed four-pass radix costs 15n against a 9n budget at every size, an
adaptive one steps between one and two passes across the probe range and reports growth
indistinguishable from real nonlinearity, and both pay ~1024 fixed operations per `finish` in
the common case where a parse records a handful of diagnostics. Those two readings are the one
thing here that is *not* remeasured — the candidate was never merged, so there is nothing left
to run — and the `9n` they are scored against is the pre-round bound `3 × (events + gap
tiles)`, from before the sort was charged its own `k log k`. The rejection stands as a decision
of that date, not as a measurement of what ships. It remains a candidate with its own
measurement.

- `finish_error_dense` — **17.4× faster** (4 096 alternating error/token pairs; 2 406 µs →
  138.0 µs). The two-pass shape read 15.9×, which is the figure this section carried before.
  A growth probe pins the shape rather than the constant: the in-suite replay-work instrument
  reads **1 299 / 5 999 / 27 199** units at n = 100 / 400 / 1 600 against its bound of
  1 600 / 7 200 / 32 000, growth **4.62× / 4.53×** for a 4× input. The 799 / 3 199 / 12 799
  against 900 / 3 600 / 14 400 at a flat **4.00×** published here before is withdrawn twice
  over: 4.00× was an artifact of charging the sort a flat element count, which made the
  charged quantity linear by construction and left the growth clause unable to fail for the
  reason it existed, and the payload predates the fold besides. A reading above 4.00× is the
  correct one — it is the sort's `k log k` becoming visible to its own gate, which is this
  section's **O(n log n)** being met rather than contradicted.
- `finish_wrap_heavy` — **3.90× faster** (2 048 retro-wrap targets; 160.4 µs → 41.1 µs). The
  two-pass shape read 3.62×.
- `finish_clean` — **8.7% faster** (74.1 µs → 67.7 µs over 8 192 tokens), where this section
  said *18% slower, and this ships*. It is the harness's designated no-regression control, and
  measured on what ships it does not regress: the gather pass was the cost, and folding it into
  the walk gives back **17.8%** against the two-pass shape — more than the regression it was
  disclosed for. What is *not* settled is the constant. The regression this replaces moved
  between +12.4% and +22.8% across the nine alignment residues of a padding sweep, and this
  remeasurement is one residue on one toolchain; composing the two puts the shipped walk
  between ~8% faster and within a point of parity across that whole band. So the 18% is gone
  at every residue the sweep covers, and the 8.7% is not a constant to lean on. `finish`'s
  internals are private, so a sharper figure can land in a patch release without breaking
  anyone.

  Method, and it is the one the scan-path table below uses. Four `[profile.bench]` builds off
  one shared `Cargo.lock`, so criterion and every other dependency is identical and only
  tokora's own source differs: the shipped tree twice in separate target directories, the
  two-pass shape at `868eb0c`, and the pre-round shape at `548fd9a`. `benches/cst.rs` is
  byte-identical across the first three. It did not exist at `548fd9a`, so that arm runs the
  shipped bench under three mechanical API adaptations and nothing else — `Sink::new` arity,
  `finish` taking the source, `cst_finish` arity — and what qualifies it is that it reproduces
  the two published two-pass ratios independently, at 15.96× and 3.57× against 15.9× and 3.62×.
  Arms were interleaved within each of sixteen rounds rather than run in blocks: 32 shipped
  runs per id against 16 two-pass and 12 pre-round. **The null control is 0.12–0.28% with no
  excursion** — the two shipped builds are byte-identical, same SHA-256 — and the smallest
  delta above clears its own control by 31×, with no population overlapping any other. This was
  **not** a quiet machine and nothing here depends on it having been one: the same M4 Pro at a
  1-minute load average of 2.5–5.0 across 14 cores, unrelated work resident throughout.

  — *(R8, #123; figures remeasured against the shipped fold)*

- A nested rollback is **linear in lineage depth** rather than quadratic.
- **The release-level scan path regresses on `next_drain`: +2.7% against 0.7.3.** The leading
  suspect, not an established cause, is the ownership that closes item 9: the benched path lexes
  with no peek, so there is no stream slot to borrow the token from, and tracking each handover
  separately — which is what makes the unwind edge correct — is state the scan must carry, and
  could not be relocated even if confirmed. It stays a suspect because nothing isolates it:
  `ScanScope` is what makes the unwind edge correct, so there is no build with it reverted to
  difference against, and the figure below is the release-level delta, 0.7.3 → 0.8.0 over 54
  commits, not item 9 alone. That absence of an isolating build is exactly why the attribution
  stops at suspicion rather than cause.

  What is measured and solid is code size: the `input_scan` bench closure grows **+560 bytes
  against 0.7.3**. An earlier draft of this entry also cited a 356-byte
  `drop_glue::<ScanScope>` symbol as evidence; that citation is withdrawn. `skip_trivia_next` is
  the only bench in this file that ever touched `ScanScope`, and item 54 below (#154) took the
  complete-input trivia skip off the shared scanner: no scan scope is built on that path any
  more, so there is nothing left to link, let alone size. `nm` confirms zero occurrences under
  `[profile.bench]` as shipped, under that profile with LTO disabled, and under an unoptimized
  debug build alike. No build configuration reproduces the symbol, so the figure is dropped
  rather than requalified. Two mitigations were tried and both reverted on measurement —
  outlining the whole `Drop` as cold made a second bench worse.

  The wall-clock figure earlier revisions of this round left owed is measured, and the whole
  `input/scan` group is given rather than the one id, because the group does not move together:

  | `input/scan` id     | 0.7.3    | 0.8.0    | change       | null control | separation       |
  |---------------------|----------|----------|--------------|--------------|------------------|
  | `next_drain`        | 333.1 µs | 342.1 µs | **+2.7%**    | 0.08%        | disjoint         |
  | `try_expect_hits`   | 216.3 µs | 219.1 µs | inside noise | 0.63%        | inside the floor |
  | `peek1_then_next`   | 530.4 µs | 527.9 µs | inside noise | 1.55%        | inside the floor |
  | `skip_trivia_next`  | 338.9 µs | 279.3 µs | **−17.6%**   | 0.25%        | disjoint         |
  | `try_expect_misses` | 523.2 µs | 427.5 µs | **−18.3%**   | 0.02%        | disjoint         |

  Two `[profile.bench]` builds — `468f7aa` (0.7.3) and this release — resolved from one shared
  `Cargo.lock`, so every dependency version including criterion is identical and only tokora's
  own source differs. The drivers and the `synthetic_source` fixture are byte-identical at both
  revisions. Arms were interleaved within each round rather than run in blocks, across three
  campaigns: 28 runs per id on 0.8.0 against 14 on 0.7.3 for the first three, 12 against 6 for
  the rest.

  **The noise floor is 0.6%, with one excursion to 1.55%**, and it was established before any
  delta was believed: a null control of two builds of *identical* source in separate target
  directories — which came out byte-identical, same SHA-256 — carried through every campaign as
  a third arm. Its two halves differ by 0.02–0.63% on five of the six ids. On the sixth they
  differ by 1.55%, because a single run of `peek1_then_next` returned 622.8 µs against a ~525 µs
  population. That is a machine event, and it is why nothing here under ~1.5% is called a
  result. `next_drain`'s +2.7% is 34× its own control, the two populations do not overlap at
  all, and it reproduced independently at +2.8% and +2.5% in two campaigns.

  This was **not** a quiet machine and nothing above depends on it having been one: an M4 Pro at
  a 1-minute load average of 2.7–4.8 across 14 cores, with unrelated work resident throughout.
  Interleaving plus the null control is what makes that survivable, since drift lands on all
  three arms alike — and the one event that did occur is the 1.55% row rather than something
  folded into an average.

  Two limits on reading it. The first is stated where the attribution is made, above: the
  figure is release-level, not item 9 in isolation, and a narrower attribution is a patch
  release's job for the same reason `finish_clean`'s residual above is. The second: earlier
  revisions disclosed **+1.5% on `skip_trivia_next`** and then withheld it as stale; withholding
  it was right for a stronger reason than staleness, because against 0.7.3 that figure is
  **wrong in sign**. #154 took the trivia skip off the shared scanner after item 9 landed, and
  the id is now 17.6% *faster*. `failed_sync_through_over_8`, offered here before as the
  counterweight at 1.4–2.5%, comes back inside noise against 0.7.3 on this harness, so it is
  withdrawn as one. The counterweight that survives measurement is `try_expect_misses`, at
  −18.3%.

  — *(R9)*

51. **`Sink::finish` replays the event log once instead of twice.** Materialization used to make
   a full gather pass over every event before the walk that drives the builder: it validated
   kinds and retro-wrap targets, collected the recorded lexer-error spans, and read the
   uncovered runs off the token spans so the walk could tile a run at the token it trails. All
   three now happen inside the walk.

   The canaries move to the arm of the event they were already about. The error spans are still
   merged into a **set** and the coverage verdict is still order-independent — it is simply
   decided after the walk instead of before it, which it can be because
   [`UncoveredGap`](https://docs.rs/tokora/latest/tokora/cst/enum.FinishError.html) was only
   ever consumed at the end. The run lookahead is now a *monotone cursor*: a run's tile is armed
   at the token that opens it and forced at the next builder-visible event, which resolves its
   far end by scanning forward to the next token — never rescanning, and skipping outright every
   region whose run the following token resolves for free.

   Measured on a 57.7 KB GraphQL document (64,085 events): **1,727 µs → 1,663 µs, −64 µs**,
   with the produced tree byte-identical across the clean, perturbed, hand-broken and
   every-prefix corpora.

   **One behaviour changes, and only on a malformed stream: error precedence.** Two passes meant
   every gather-class refusal (`ReservedKind`, `StaleStartAt`, `DanglingForwardParent`,
   `InvalidDiagnosticSpan`, `InvalidDialectKind`) outranked every walk-class refusal
   (`OrphanFinish`, `ImproperWrap`, `MismatchedFinish`, `OverlappingSpans`, `SpanOutOfBounds`,
   `OffsetOverflow`) whatever their buffer indices. One pass reports the **first violation in
   buffer order**. No stream that was refused is now accepted, and none that was accepted is now
   refused; a stream with two defects may name the other one. Within a single event the order is
   unchanged — each fused arm runs its gather checks ahead of its walk checks.

52. **A `Sink` reserves its event log at construction, from the source's length.** The log used
   to grow from empty, doubling about sixteen times over a 57.7 KB document and copying roughly
   4 MiB in the process. `Sink::new` now asks for the capacity that doubling would have arrived
   at — the source's byte length rounded up to a power of two, capped at 65,536 events (2 MiB) —
   so the same final block is bought once.

   The predictor is sound only because a sink is compile-time restricted to trivia-surfacing
   lexers: every source byte reaches it as a token or a reported lexer error, so the event count
   tracks the byte count (0.80–1.11 events per byte across this crate's corpora). The **cap** is
   what keeps that from being a liability for a grammar whose tokens are long — past it the byte
   count stops being evidence and the `Vec` resumes doubling — and the **rounding** is what keeps
   the reservation from backfiring: reserving the raw length under-reserves a lossless log, which
   buys a large eager allocation *and* a double-sized reallocation on top, and measured slower
   than reserving nothing at all. An empty source still allocates nothing.

   Measured on the same 57.7 KB document, paired over fourteen rounds: **−5 µs (σ 8)**. The
   allocation count and the ~4 MiB of copying go regardless of what the wall clock on one
   allocator says.


53. **A `trace`-feature preview no longer costs the entire remaining source, every time.**
 `InputRef`'s per-event source preview (`trace_preview`, behind the `trace` feature) built
 `format!("{rest:?}")` over the *whole* remaining source — potentially the entire input, at
 every instrumented combinator call (`peek`, `begin`/`commit`/`rollback`, `try_expect`,
 several per token) — then walked that string a second time to count it, just to keep the
 first 24 characters. Θ(remaining input) per event, unconditionally, made Θ(n²) over a parse:
 benchmarking with `--all-features` (which enables `trace`) against a ~128 KiB fixture wrote
 133 MB of stderr in 5 minutes without finishing the cheapest bench cell's warm-up.

 A bounded `fmt::Write` sink now aborts the `Debug` call as soon as it has enough output to
 answer the 24-character window and the ellipsis, so the preview never walks more of the
 remaining source than a `Debug` impl's own internal batching forces. Measured against this
 crate's own `input_scan` bench fixture shape (newline-delimited, digits and punctuation): a
 single preview at offset 0 goes from 250µs to 208ns at 128 KiB and from 10.5ms to 958ns at
 8 MiB; a full traced parse over that shape scales linearly in token count on both sides of
 the fix, at a per-token cost that no longer grows with how much input is left.

 That bound is conditional, not universal: it holds for a `Debug` impl that writes
 incrementally, and it does not hold for one that front-loads its entire output before its
 first write. Every `Source`/`Slice` pairing this crate ships was checked against that line;
 the guarantee is asserted for exactly these, not for `Source` in general.

 A third shape is not narrow at all, and is the only one of the three reachable from outside
 this crate. `Source`'s `Slice` associated type is public and implementable downstream, and
 `Slice`'s own bound (`PartialEq + Eq + Debug`) puts no streaming requirement on its `Debug`
 impl. A conforming impl may scan, escape and allocate its *entire* remaining slice into a
 temporary and call `write_str` once; this sink cannot shorten work that already ran before
 it was ever invoked, so the cost stays exactly the `O(remaining source)` this fix exists to
 remove. Output is still correct even here — the window and the ellipsis decision come from a
 genuine prefix of the untruncated dump no matter how it was produced — so this is a cost
 gap, not a correctness one, but it is the most severe of the three: unbounded, not merely
 narrower, and reachable with an ordinary trait implementation, no unsafe or misuse of this
 crate required. No in-tree backing does this today; `bounded_debug`'s doc comment and its
 `bounded_debug_is_correct_but_unbounded_for_a_front_loading_debug_impl` test carry a fixture
 that exercises exactly this shape.

 The other two remain narrower and **found, not fixed**: a `[u8]`-shaped backing (`[u8]`,
 `HipByt`, the `smol-bytes` byte types) bounds its allocation and escaping work but still
 iterates every remaining element, because `Formatter::debug_list` drives its iterator to
 completion independently of write failures; and a `str`-shaped backing (`str`, `HipStr`,
 `Utf8Bytes`) still costs `O(run)` for a run of characters with nothing escape-worthy in it,
 because `Debug for str` accumulates such a run before its first write. Realistic text —
 which breaks such runs at newlines, quotes, etc. — is unaffected, but a contrived input like
 this crate's own `int_run_source`/`comma_list_source` bench fixtures (digits and spaces for
 ~128 KiB at a stretch) is not; `bytes::Bytes` and `bstr::BStr` have neither gap, since both
 hand-write their `Debug` loop with `?` on every write.

 Output is unchanged: every trace line reads identically to before this fix, character for
 character, ellipsis included — checked against a spread of inputs (empty, short, exactly at
 the 24-character boundary, far longer, and content that escapes heavily: newlines, tabs,
 both quote characters, backslashes, non-ASCII, and a multi-byte scalar straddling the cut).

54. **The trivia skip no longer goes through the shared scanner on a complete input.**
   [`skip_while`](https://docs.rs/tokora/latest/tokora/input/struct.InputRef.html#method.skip_while) —
   the primitive behind the `padded` combinators, and the door every lossless grammar opens at
   every decision point — now runs its own two-phase loop under
   [`Complete`](https://docs.rs/tokora/latest/tokora/input/struct.Complete.html): a token already
   at the front of the stream is judged **where it lies** and consumed there, and only once the
   stream is empty does it reach the lexer. No scan scope is built, no frontier pair is cloned,
   and a token already resident is never taken out and put back to be judged. The lexing phase
   does keep one thing the scope carried, and entry 59 above is that.

   **Measured against the previous release line: 7.4% off a whole GraphQL parse** (1600.2 µs →
   1482.5 µs on a 57 kB document; 159.0 µs → 148.1 µs, 6.9%, on a 7.5 kB one). Minimum over nine
   blocks with `apollo-parser` as an unchanged control, builds interleaved within each repetition,
   five repetitions, contended repetitions discarded. That figure is **net of** the unwind guard in
   entry 59 above, which is the honest number for this release because the guard ships with the
   route: without it the same measurement reads 17.4% and 15.9%. An earlier draft of this entry
   claimed 25%; that did not describe this measurement even before the guard existed, and it is
   replaced rather than adjusted.

   **No caller-visible behaviour changes**, and the guarantee is the one already documented on the
   method: for a caller whose input-layer callbacks are inert and whose predicate answers from the
   values it is handed, the parse result, the tokens read next, the resume cursor, the committed
   span and lexer state, the diagnostics, the poison boundary, the dedup watermark and the
   predicate call sequence are all identical. What the route runs *internally* differs: it clamps
   the committed position once per skipped token where the scan clamped once per call, and it asks
   the cache for its front where the scan popped and pushed back.

   Two things it had to re-establish, both pinned by new cells in `fast_path_tests`. A resource
   trip inside a skip latches the poison boundary at the **committed cursor** rather than at the
   scanner's deferred frontier, and those are the same offset because the route commits every
   token as it crosses it. And a panic mid-skip leaves the input whole: a call interrupted at its
   `k`-th predicate consumes exactly the `k − 1` tokens the predicate accepted and every other
   token stays reachable. The two phases reach that differently, and entry 59 above is what makes
   the second half true — the resident phase runs the fallible half of each settle while the token
   is still in the stream and so needs nothing, while the lexing phase holds the token it lexed
   across the predicate and owes a put-back. One consequence is visible only to a host that catches
   an unwind: an interrupted end-of-input commit now keeps the prefix the call had already crossed
   (span and state describing the same token) where the scanner discarded it.

   A [`Partial`](https://docs.rs/tokora/latest/tokora/input/struct.Partial.html) input keeps the
   shared scanner, whose scope owns the five-fact `Incomplete` restore and the emitter mark a
   streaming skip needs settled on every exit. The split is by typestate and is guarded by a
   differential sweep: a *sealed* partial input takes every decision a complete one takes, so the
   two routes are held to the same observation over the same programs, sources and cache
   capacities.

   **That parity claim excludes exactly one exit, and the exclusion is deliberate**: an unwind
   *inside the end-of-input settle* — the consequence the paragraph before last already names, seen
   from the other side. Both routes finish a skip that ran to end of input the same way — read `Lexer::span`,
   take `Lexer::into_state`, write the pair — but the scanner reaches that settle having already
   disarmed its scan scope, so the frontier holding the whole skipped run is dropped with the
   unwind, while this route committed each token as it crossed it and keeps them. The complete
   input's answer is the better one: both routes told `Emitter::commit_token` that the run's tokens
   were consumed, and only this one's committed position agrees with what it said. Every callback
   that can raise that unwind — the lexer, the source, the span, the offset — is one the method's
   own precondition already requires to be inert, so the narrowed claim costs a caller who meets
   the condition nothing. Nothing wider diverges: a panic out of the predicate, out of
   `Emitter::commit_token`, out of the emitter's diagnostic path, out of `Lexer::lex`, or out of
   `Lexer::span` anywhere but the settle leaves the two routes identical, as does every run in
   which nothing unwinds. The difference is pinned — as a difference, with both columns asserted —
   by `the_two_completeness_routes_are_pinned_apart_on_an_interrupted_eof_settle`.

### What CI enforces now that it did not at 0.7.3

A reader deciding whether to upgrade trusts gates, not adjectives. Every one of these was
added because something got through the previous set.

- **The changelog has a structural check.** It is the one load-bearing release artifact no
  compiler, linter or test reads, and it had already failed in production: a rebase resolved it
  as a verbatim union — textually correct, no conflict markers — and silently ate a heading,
  filing six breaking changes under `### Performance` while every gate passed. The check runs
  in seconds with no toolchain and is a merge-blocker, not advisory.
- **The `no_std` tiers execute.** The previous job *built* the crate without `std`, which never
  compiles a test target — so nothing `no_std`-shaped had ever run. Enabling it found a failure
  that predated the change and that no gate could have seen. That configuration now runs
  **its tests where it ran none**, and the CAS-less backing is checked on a real CAS-less
  target.
- **The compile-time diagnostic rails assert they ran.** The trybuild harness pins its
  `.stderr` files to the MSRV toolchain and returns early on any other rustc — passing, having
  compiled nothing. A skip was indistinguishable from a pass. It now reports an executed-case
  count, the CI step asserts the count rather than the exit status, and a separate check
  compares the case directory against the harness's registry by exact set equality, because a
  floor cannot detect a missing member.
- **The docs are built without default features, not only with all of them.** An all-features
  doc build documents every item, so a link to a feature-gated target always resolves there —
  it is structurally incapable of seeing a broken link in a smaller configuration. The bare
  build had never run, and it was failing outright with 39 unresolved links. Acceptance is the
  exit status, not a diagnostic count: rustdoc aborts early, so a count is a floor and a floor
  cannot detect a missing member.
- **The per-version logos legs actually exercise the adapter.** The `logos parity (0.14, 0.15)`
  matrix job existed, but the crate's 79 adapter integration files were gated on a feature that
  implies 0.16, so they were invisible to it — a green gate over an untested configuration.
  Those gates now name all three versions, and the same job exercises the adapter on each.
- **A guide-only change is no longer un-gated.** The guide chapters are Markdown compiled as
  doctests, so a blanket `**.md` path filter meant a guide-only pull request landed with zero
  CI.
- **The public API is diffed mechanically against the last published version.** Recall-shaped
  sources — pull-request bodies, commit messages, surface greps — are structurally blind to
  bound additions and to derived-`Debug` movement, and in this release two of those three
  sources turned out not to exist at all for the earliest rounds. The mechanical diff found two
  breaking changes that would otherwise have shipped undisclosed. Advisory-red, because it
  rides rustdoc-JSON on a pinned nightly and can redden without the crate changing.
- **Runtime parity across the three supported logos majors.** The crate advertises 0.14, 0.15
  and 0.16, and until now the per-version matrix only ever *clippy-checked* them — no test had
  executed against 0.14 or 0.15 once. The first run of these cells was red, on a dead-code
  failure that predates this round: an odometer module gated on `std` whose only readers are
  gated on `logos`, so a `std, logos_0_N` test build had two unused functions and
  `#![deny(warnings)]` refused it. **That is the second never-executed configuration this round
  found broken on arrival**, after the `no_std` one. Both now run and pass. *(The adapter's own
  test module is still hard-coded to 0.16, so those adapter-level tests remain single-version.
  That gap is real, named, and not closed here.)*
- **The deep fuzz sweep runs per pull request.** Seeds `0..50_000`, `#[ignore]`d since it was
  written and never run anywhere. It was expected to need a monthly cadence; measured, it costs
  **0.10s in release** (0.85s in debug — the ratio is what says it is doing the work), so the
  whole cost is a build this job has already paid.
- **Miri covers the `unstable-raw` surface.** The raw twins are the surface two rounds
  reordered around capture windows — precisely the unsafe-adjacent shape Miri exists to see —
  and no configuration Miri interpreted before this release enabled them. The second pass runs
  the lib unit tests *and* the whole integration suite again under `logos,unstable-raw`; both
  Miri matrices are sharded by test target, so that cost is spread across shards rather than
  serialised into one cell.
- **The sanitizer script has a caller.** `ci/sanitizer.sh` existed, worked, and was referenced
  by no job — so it had never run on any branch of this campaign. That is the same class as a
  configuration CI compiles and never executes, one level up: not a gate that covers nothing, a
  gate that nothing invokes. ASan and TSan are now one matrix cell each, and the target moved
  from a constant inside the script to an argument the workflow passes, because which
  sanitizers exist is a property of the target.
- **Every public name this release adds is probed against its real owner.** One byte-identical
  consumer is compiled against the merge base and against the branch, over an inventory
  generated from both sides' rustdoc JSON rather than hand-listed, and the gate fails on the
  outcome no diagnostic reports: both sides compile, neither warns, and the witness disagrees.
  What CI does **not** do is build a real downstream crate. A job that cloned one at a pinned
  ref was written for this release and removed before it shipped — it never ran on a push or a
  pull request, and `continue-on-error` kept it from failing the build on the schedules where
  it did run — so a source-level break that only an independent consumer can witness is caught
  by that consumer's own CI, not here.

### Streaming CST is not in this release

`PartialSession::parse` now requires `Ctx::Emitter: ValueKeyedEmitter`, which makes pairing a
session with a recording `Sink` **uncompilable**. This is a deliberate scope decision with a
return path, not an omission.

The reason is that the *sound* way to use the pairing was never enforceable. A session redrives
each attempt from the base, so a threaded sink accumulates the replayed prefix; the correct
usage is a fresh sink per attempt, and **no type can observe freshness.** The choice was
therefore between shipping a combination documented as unsound and shipping a bound that makes
it unrepresentable. The bound also states something the crate already believed: `Sink`
deliberately does not implement `ValueKeyedEmitter`, on the stated ground that a sink cannot
wrap a sink.

**Diagnostics-only streaming is unaffected** — `Fatal`, `Verbose`, `Silent`, `Ignored` and
`&mut` of each all satisfy the bound, and a compile-time control pins that so a future
tightening cannot take streaming diagnostics with it unnoticed. Everything else in the
streaming lifecycle ships: the budget, the committed frontier, the terminal latch, the
refusal-coherence law.

**What returns with the witness:** the chunked-session-equals-one-shot guarantee, and the
supported fresh-sink-per-attempt story.

**The bound closes the session route only.** A hand-rolled refill loop that calls
`parse_partial` directly with a shared recording sink produces the same duplication — measured,
not inferred. `parse_partial` is unchanged: it predates this release, and a *single* call with
a sink is perfectly sound, so bounding it would forbid working code rather than only broken
code. If you hand-roll a redrive loop, build a fresh sink per attempt.

— *(R8, #123)*

### Known limitation — a parse and the emitter serving it share no identity (one witness closed, one open)

Nothing in the `Emitter` or `ParseContext` contract lets an emitter learn which parse it is
serving: `Cursor` carries an offset and a `PhantomData`, and `commit_token` carries a token and
a span. R8 filed this as an invariant with **two** witnesses and said the fix was *"one contract
with two additions"*. **One of the two landed in this same release; the other did not**, so the
statement is rewritten witness by witness rather than left describing a tree that no longer
exists.

**Witness 1 — a sink bound to a foreign source — CLOSED where an inequality is proof.**
`Emitter::bound_source` (defaulted, `None`) lets an emitter declare the source it binds, and the
point at which an emitter is attached to an input compares that against the source the parse
reads. Since #128 that point is the input's **constructor** — where the two are bound for the
input's whole life — so the comparison happens once per input and covers every borrow. A `Sink`
built over `"XY"` and driven over a same-length `"ab"` now **panics at the parse entry** instead
of materializing a tree whose text is `"XY"` and whose structure came from `"ab"`. Equal length was always the point: no length check caught it, and `finish` never could,
because the event log a wrongly-paired sink produces is byte-identical to one a legal single
parse could produce. The cell that pinned the wrong answer is now
`sink_bound_to_a_foreign_source_is_refused` and asserts the panic.

**The residue, narrowed and stated rather than dropped.** The refusal fires only where
`Source::REFERENT_IS_BYTES` says an unequal reference *proves* an unequal source — the `?Sized`
backings (`str`, `[u8]`, `BStr`), where the reference is the data. For a **sized** backing the
reference addresses a variable rather than the bytes: two `bytes::Bytes` handles cloned from one
`Arc` name the same buffer at two addresses, and refusing them would convert a working program
into a runtime panic. So for the owned handles — and for the `&str` / `&[u8]` reference forms —
the door stays open, exactly as it was. That is the honest price of refusing to break correct
code, and a third party closes it for their own backing by overriding one `const`.

What the check covers is wider than "3 of 13 `Source` impls" suggests, because the population
that matters is *declaring lexers*: every lexer in this tree declares `type Source = str` or
`= [u8]`, and the logos adapter inherits logos' own. **No shipped lexer declares a reference
form.**

**The wrapper hole is NOT closed, and four shapes reach a wrong-buffer tree.** The seam can
only compare an answer it receives, and every route below produces silence or a forged answer
instead:

- **A wrapper that hides `bound_source`.** It forwards every emission and omits that one
   method, so the seam concludes "this emitter binds no source". No bound catches it: a wrapper
   writing `impl CstEmitter for W {}` satisfies `Ctx::Emitter: CstEmitter` while inheriting
   every default, and a provided-method override is invisible to a bound.
- **A parse whose events are all diagnostics.** No token is committed, so nothing token-shaped
   exists to key on, while the lexer diagnostics' spans still license gap tiling over every
   byte.
- **Pre-arming.** `Emitter::bound_source` is a public trait method, and — before
   [68](#0.8.0-changed-breaking) made the accessor crate-private — `InputRef::emitter()` handed
   parser code `&mut Ctx::Emitter`, so any holder could query the binding. Nothing a sink records
   about "I was asked" can distinguish the parse entry from anyone else.
- **Hand-emitted `cst_token` spans.** The manual emission door carries a span, so a grammar can
   choose which source bytes the tree shows while consuming nothing.

**A finish-time wall against (1) was built during this round and removed before release.** It
caught the naive wrapper and each of (2), (3), (4) was then found to walk around it — every fix
relocated the hole, because the witness was flags on the sink and *the sink cannot tell who set
them*. Shipping a `FinishError` variant that names a protection with three known bypasses is
the same defect as a `## Panics` section promising a panic the code does not perform, and this
release deleted nine of those. All four shapes are pinned by cells asserting today's answer.

**What closes the class is minting the sink from the source, and it landed in this release
instead of 0.9 as first planned.** [68](#0.8.0-changed-breaking) ships `cst::parse_lossless` and
`cst::parse_lossless_partial` in place of the `input.sink(inner, profile)` shape sketched here:
the source is named once, in the driver's own argument list, and handed to both the `Sink` it
mints and the `Input` it drives, so the binding is neither hideable nor forgeable because there
is nothing to hide or forge. `Sink::new` is `pub(crate)`, so bullet (1)'s wrapper has no `Sink`
left to build over — which takes bullet (2) with it, since an all-diagnostic parse still needs a
wrapper to hide behind. Bullet (3) closes because the query route it used,
`InputRef::emitter()`, is crate-private now (see that bullet, revised in place). Bullet (4)
closes the way `CstEmitter::cst_token` itself does — deleted, not walled. The returned `Cst`
implements no emitter trait either, so the artefact cannot be re-aimed at a second parse
regardless of which of the four a caller tried. What this leaves open is stated below, where
this section's own "when it lands" sentence used to be.

**Witness 2 — the completeness witness — UNTOUCHED.** `CstEmitter`'s contract still names no
completeness parameter, no attempt and no input, so any holder of the emitter may append events
at any moment and the event log still cannot distinguish the live attempt from an abandoned one.
Every route is an instance of that: the `InputRef` and `ParseState` accessors that stay generic
over completeness — deliberately, because `Partial` legitimately needs them for **diagnostic**
emission — and every combinator that hands either to a user callback. Pinning individual
combinators to `Complete` walls an instance; it cannot wall the class, which is why this stays
contract work rather than a door-by-door patch.

The stale-structure witness — an abandoned attempt's node surviving into a later attempt's tree
— is closed for the session route by the `ValueKeyedEmitter` bound above, and its test was
deleted rather than weakened, because the shape it exercised no longer compiles.

**So R8's sentence still holds, and now reads "one landed, one open".** A *source* identity
could not live on `CstEmitter`, because it would miss `Sink::commit_token` — the path every
ordinary parse uses — which is why it sits on `Emitter` and is checked where an emitter meets an
input. A *completeness* witness on the same contract is what closes witness 2 and what returns
streaming CST.

**The repair landed as [68](#0.8.0-changed-breaking), and two of its three predictions here did
not hold.** This paragraph forecast that minting the sink from the input would close witness 2
at the same time and would be "the mechanism that restores streaming CST". Neither happened:
witness 2 is untouched, exactly as stated above — `CstEmitter` gained no completeness parameter
— and streaming CST is still blocked by the `ValueKeyedEmitter` bound described in *Streaming
CST is not in this release*, above, an axis minting never touches. Both were forecasts about a
mechanism that had not been built yet, made on the assumption that one constructor change would
answer two independent invariants; only the one this section is about turned out to be true.

**Retained, not retired.** The same paragraph promised that landing minting would retire
`Emitter::bound_source` and `Source::REFERENT_IS_BYTES` — *"two mechanisms for one invariant is
how a crate ends up with three."* That assumed a closure that does not hold.
[68](#0.8.0-changed-breaking)'s drivers pin `Sink` into the emitter slot **by name**, but the
value they hand a callback instead of the emitter — `EmitterView`, also new there — implements no
emitter trait only as a fact **about this crate**. Under the orphan rules, a downstream crate
whose own lexer type appears in the parameter list may write
`impl Emitter for EmitterView<'_, '_, ItsLexer, Sink<…>>` and install that over a second, foreign
buffer from inside a callback — a route through a callback parameter, which is exactly what
minting's pinned-slot trick does not reach, because that trick only pins the driver's own entry.
`Emitter::commit_token` is off the view's surface there too, so the route still carries no token,
no span and no byte of the foreign buffer — but it can carry a forged or absent `bound_source`
answer, which is bullet (1) above, one level in. The handshake is what a wrapper on that route
has to forward honestly for the general constructor check to still catch the foreign pairing;
a wrapper that declines to forward
it reports `None` and the seam sees nothing to refuse — the same residual disclosed for every
wrapper since R10. And nothing about `bound_source` names `Sink` or `cst` in the first place: it
is the general `Emitter`/`ParseContext` handshake, so any hand-rolled emitter paired with the
ordinary `parse`/`parse_partial` entry points depends on the same check regardless of CST.
`bound_source`, `REFERENT_IS_BYTES` and `SourceIdentity` earn their place on the route minting
cannot reach, and stay additive infrastructure rather than a stepping stone to their own
retirement.

Refusing any of this at materialization remains **impossible**, not merely expensive: the bad
event logs are byte-identical to logs a legal single parse could produce, so `finish` has
nothing to discriminate on. That is why the check has to sit at the seam or nowhere.

— *(R8, #123; witness 1 narrowed by R10, its construction-time route closed by #168; witness 2
still open)*

### Known limitation — `finish` does not refuse duplicate zero-width token spans

Separate and smaller, listed apart because it is **not** the identity gap: the monotone-span
wall never fires when duplicated spans are both `[0,0)`, so such a log materializes. A
conforming lexer cannot produce one — a token is a span of consumed bytes, and a lexer
returning zero-width tokens without advancing does not terminate — so this is reachable only
through the raw event surface or a third-party `Lexer` that ignores the contract, which is
what the sink's release walls exist for. It pins a property of *materialization*, not of the
parse-to-emitter contract, and it flips when `finish` starts refusing these spans. —
`duplicate_zero_width_tokens_are_not_yet_detected`

— *(R8, #123)*

<a id="0.8.0-known-limitation-recorded-and-closed-before-release--a-discarding-error-sink-erased-the-recursion-trips-stop-not-its-bound"></a>

### Known limitation recorded and closed before release — a discarding error sink erased the recursion trip's stop, not its bound

**This limitation was closed before release and does not ship in 0.8.0.** It was recorded when the
recursion budget landed and answered later in the same release by [48](#0.8.0-changed-breaking),
which moved the resource-trip witness off the grammar's error value and onto the input session,
where no `From` the grammar writes can reach it. The heading above originally read as a live
caveat despite this paragraph, so it was rewritten to state the closure directly; the anchor
changed with it, since this file names an anchor for the heading text it sits above, and the
in-document link from item 48 was updated to match. Design documents written while the
limitation was open still cite the old anchor and are outside this file's reach. What follows
is kept as the record of what it was, in the past tense, and not a caveat a 0.8.0 consumer has
to act on.

**What it was.** Item 42's `RecursionLimitReached` is always terminal, and the recovery combinators
decided whether to re-raise by asking the **converted** error — the type the grammar actually
names — for `MaybeTerminal::is_terminal()`. So a grammar whose `From` for it discarded the value,
`()` included, got `false` back and **recovery spent the trip** instead of re-raising it: measured
on one ladder, one limit and one 32-deep input under `skip_then_retry`, the surrounding grammar was
handed back offset **68** — the whole chain and the sync token consumed and committed — where a
delegating error type was handed back **0**. That was the pre-existing `MaybeTerminal` opt-out
reaching a *resource guard* rather than a malformed-input report, and this entry recorded it rather
than answering it: 16 error types under `error::` carry a `From<…> for ()`, 14 of them before this
release, so it read as two more instances of a crate-wide design rather than a new one.

**What it never put at risk — measured, not reasoned, and still true:**

- **The native stack is already back.** `Descent`'s destructor releases every level on the unwind
  that carries the error out, so by the time any recoverer sees the converted value, every frame
  the budget was protecting has returned. On a 200-level trip the recoverer's frame sits **160
  bytes** from the pre-parse baseline in a debug build and **0** in a release one, against descents
  that reached ~1 MiB and ~97 KiB. The guard's stack-safety purpose survives the sink intact.
- **The depth budget is unchanged.** The cell reads back to its pre-parse value, so a recoverer
  that spends a trip and descends again starts from the same depth and meets the same limit.
  Re-tripping is bounded, not compounding.
- **The retries terminate**, on their own zero-progress guards rather than on terminality:
  `skip_then_retry` consumes its sync token per continuing cycle and runs out of sync points, and a
  repetition over a recovering element stalls on the first element that commits nothing.

**How it is answered.** Item 48 carries the full account and the before/now measurements. The short
form: the witness is a monotone counter on the **input session**, bumped by `InputRef::descend`'s
trip arm *before* the grammar's `From` runs, and `recover`, `inplace_recover`, `skip_then_retry`
and the resilient collection loops read it **beside** `MaybeTerminal::is_terminal`. Both error
types now hand the surrounding grammar back offset **0**. A resource bound that an unrelated error
sink can opt out of is not a bound.

**What the answer does not cover, and what is still open.** `NonAssociativeChain` was the control
here and remains inert: it is `is_terminal() == false` to begin with, so recovery spends it through
a delegating type and through `()` alike and the sink decides nothing; what a sink costs there is
only the offset, which is the ordinary price of discarding a payload and not a change of contract.
And the *class* is older than this release and still stands. `UnexpectedEnd` — the type behind
`UnexpectedEot`, `UnexpectedEoLhs` and `UnexpectedEoRhs`, whose terminal values are real driver
output — is the only other type under `error::` that overrides `is_terminal`, it carries a
`From<…> for ()` of its own, and both have shipped since before this campaign. Whether a discarding
sink should be able to erase terminality for a grammar error, as it no longer can for a resource
guard, is a question about `MaybeTerminal` rather than about these two types, and it is still
recorded rather than answered.

— *(R16; closed by #148 R1)*

### Notes

`#[diagnostic::do_not_recommend]` was evaluated and rejected. It does not change whether the
curated message appears; its only effect is to hide *which* member is missing, which is the
one datum a consumer needs. Measured on 1.87 and on nightly.

— *(W-api-A, #118)*

**`finish_*` unification attempted and stopped, on the close predicate and close-miss slots.**
`parens`/`braces`/`brackets`/`angles` and `delimited::<D>` share their close body already
(`commit_delim_close`); what the four named parsers duplicate is the five slots they fill it
with. Folding them onto the generic path stops on the two slots that ask the same question
through different traits: the named route asks the **token** (`PunctuatorToken`'s
`close_paren() -> Option<Kind>`), the generic route asks the **pair marker**
(`Punctuator::kind()`, which needs `<L::Token as Token>::Kind: From<CloseParen>`). Neither trait
implies the other, so the fold does not compile without adding `Kind: From<OpenX>` and
`Kind: From<CloseX>` to the four public named parsers — measured, four `E0277`s. Unification here
is signature-stable or it does not happen; the two routes stay disclosed, and 0.9 is equally
breaking-capable if the bounds are ever worth taking.

Their agreement is now pinned rather than assumed (`tests/delimited_route_parity.rs`): the close
predicate over every token kind × all four pairs, the close-miss error compared as a value, and a
16-row corpus — 4 pairs × {unclosed-at-EOF, wrong-closer, wrong-opener, nested-unclosed} — parsed
both ways under a recording emitter. **No render deltas: all 16 rows are identical**, diagnostics
and final cursor alike. The suite exists because nothing in the type system ties a token type's
two punctuator declarations together, so a type whose declarations disagreed would send the two
routes to different diagnostics silently.
— *(R11)*

## 0.7.3 (2026-07-24)

### Added

- `FusedDispatchOnKind` now implements `TryParseInput`: a table hit remains lex-once, while a
  table miss or end-of-input returns `ParseAttempt::Decline` without consuming valid input.

## 0.7.2 (2026-07-24)

### Added

- **Fallible projection traits.** `tokora::utils::Downcast<T>` and
  `DowncastRef<T>` provide owned and borrowed `Option<T>` projections for domain
  types without imposing blanket implementations or a concrete conversion model.

### Fixed

- **Wrong opening delimiter at end of input is no longer misreported as end-of-input.** The
  delimited many-drivers (`repeated`, `repeated-while`, `separated`, and `separated-while`
  `…delimited::<D>()`) now report a wrong opening delimiter as the expected-open unexpected-token
  diagnostic carrying that token, even when it is the final token — instead of `UnexpectedEot`.
  The wrong token stays unconsumed, and the diagnostic no longer depends on whether another
  token happens to follow it. `UnexpectedEot` is now reserved for a genuinely empty opener
  position. Fixes issue #85.

## 0.7.0 (2026-07-23)

### Added

- **Lifetime-preserving borrowed sources.** `Source<usize>` is now implemented explicitly for
  `&'data str`, `&'data [u8]`, and (with `bstr_1`) `&'data BStr`. Their associated slices retain
  `'data` even when the source value itself is borrowed for a shorter call.

### Changed (breaking)

- **`Slice<'source>` now guarantees validity for `'source`.** The trait has a
  `+ 'source` supertrait requirement. Canonical implementations live on `str`, `[u8]`, `BStr`,
  and the optional backend value types; a shared-reference blanket implementation forwards
  `&T`, including nested references, without changing the representation or iterator behavior.
  External `Slice` implementations must ensure that the slice value and its represented data
  outlive `'source`.
- **Borrowed backend slices preserve their representation and longest available lifetime.**
  `BStr` sources now yield `&BStr` instead of `&[u8]`. `HipStr<'data>` and `HipByt<'data>`
  sources now yield `HipStr<'data>` and `HipByt<'data>` rather than shortening the associated
  lifetime to the method borrow.
- **`Source` references are intentionally explicit rather than blanket-forwarded.** This avoids
  shortening a lifetime carried by the source through an unrelated outer borrow. Owned backends
  such as `Bytes`, `HipStr`, and the smol-bytes types remain `Source` implementations on their
  owner types; borrowing an owner to call its methods remains unchanged.

### Migration

- Update custom `Slice<'source>` implementations so the implementing type satisfies
  `'source`. Prefer implementing `Slice` on the canonical representation and let the shared
  reference blanket provide `&T`.
- Code that names `<BStr as Source<usize>>::Slice<'a>` as `&'a [u8]` should use `&'a BStr`
  instead; call `AsRef::<[u8]>::as_ref` when a byte slice is specifically required.

## 0.6.2 (2026-07-23)

### Changed

- `Ident`, `Keyword`, and the literal carriers now accept unsized source/data types.
  Borrowed accessors are available for carriers such as `Ident<str>`, `Keyword<str>`, and
  `LitDecimal<str>`; construction and other by-value operations remain limited to sized
  source/data types. Unsized language markers are now supported consistently by generated
  literals, `Ident` recovery, and `IdentList` APIs; the established public generic order is
  unchanged.

## 0.6.1 (2026-07-22)

### Added

- **Method-form delimiter combinators.** Every `ParseInput` parser can now use
  `delimited::<D>()` directly, with `delimited_by_parens`, `delimited_by_braces`,
  `delimited_by_brackets`, and `delimited_by_angles` as named conveniences.
- **Named delimiters for many-builders.** The same delimiter method family is available on
  `Repeated`, `RepeatedWhile`, `Separated`, `SeparatedWhile`, `AtLeast`, `AtMost`, `Bounded`,
  `AllowLeading`, `AllowTrailing`, `RequireLeading`, and `RequireTrailing`. Existing
  repeated/separated `delimited::<D>()` chains remain source-compatible, including nested
  cardinality and separator-policy wrappers.

## 0.6.0 (2026-07-22)

### Changed (breaking)

- **Delimiter-specific `Unclosed` diagnostics.** Delimited shape and many parsers now emit `Unclosed<D, …>` using the delimiter marker directly; built-in shapes use their concrete `Paren`, `Brace`, `Bracket`, or `Angle` tag. Consumer error types should provide the corresponding `From<Unclosed<…>>` conversions.
- **Composable emitter bundle includes unclosed diagnostics.** `ComposableEmitter`, and therefore `ParseCtx`, now includes `UnclosedEmitter`; custom composite emitters must implement it.
- **Built-in delimiter markers are language-neutral.** Bare `Paren`, `Brace`, `Bracket`, and `Angle` work with any parser `Lang`; the marker language no longer has to match the parse language.

## 0.5.1 (2026-07-21)

### Added

- **Full smol-bytes integration** — `Source`, `Slice`, `Equivalent`, and `ToEquivalent`/`IntoEquivalent` impls for all four `smol-bytes` types: `shared::Bytes`, `compact::Bytes`, `Utf8Bytes` (`shared::Utf8Bytes`), and `compact::Utf8Bytes`. `Source`/`Slice` treat the byte types as `u8` sequences and the UTF-8 types as `char` sequences with UTF-8-boundary-aware indexing; `Equivalent` compares all four types via their byte view, mirroring the existing `bytes`/`hipstr` impls; `ToEquivalent`/`IntoEquivalent` convert the byte types from `[u8]`/`&[u8]` and the UTF-8 types from `str`/`&str`. Feature-gated behind `smol_bytes_0_1`.

## 0.5.0 (2026-07-20)

### Added

- **`Source::as_slice`** — returns the entire source as its `Slice` associated type (the same contents as a full-range `slice(..)`). Element-agnostic across `str`, `[u8]`, `Bytes`, `HipStr`/`HipByt`, smol-bytes `shared`/`compact`/`Utf8Bytes`, and `BStr`; owned sources share the whole source via `clone`, borrowed sources return `self`/`as_ref`.

### Changed (breaking)

- `Source::as_slice` is a **required** trait method (no default body): any external `Source` implementor must now provide it. This is the source-breaking change behind the 0.5.0 bump.

## 0.4.0 (2026-07-20)

### Fixed

- **Unterminated delimited many-builders now report the opener as `Unclosed` through the
  emitter instead of silently accepting the input.** A delimited many-builder
  (`item.repeated(…)`, `item.repeated_while(…)`, `item.separated_by_*(…)`, or
  `item.separated_by_*_while(…)` closed with `.delimited::<D>().collect()`) driven over input
  whose closing delimiter never arrives before end-of-input — e.g. `"(1 2"`, `"[1 2"`, `"{1 2"` —
  used to return `Ok` with the elements parsed so far. It now emits an `Unclosed` diagnostic
  carrying the **opener's span** and the delimiter pair's name:
  - under a fail-fast `Fatal` emitter the parse fails with it (via the `From<Unclosed<…>>`
    conversion);
  - under a recovering `Verbose` emitter the diagnostic is recorded and the parse recovers,
    yielding the elements collected so far.

  A wrong token where the closer belongs still reports the existing unexpected-token
  (expected-close) vocabulary. The `separated`+delimited driver — which previously *did* error
  at end-of-input, but with a stale unexpected-token pointing at the last element rather than at
  the opener — now reports `Unclosed` at the opener like the other three drivers.

  The close-status diagnostic is the **primary**: the `separated`/`separated_while` delimited
  drivers emit it **before** the end-state secondaries (`TooFew`, separator policy), so a
  fail-fast emitter fails with `Unclosed` on e.g. `[` under `at_least(1)` or `[1,2,` at
  end-of-input rather than letting the secondary short-circuit it, and a recovering emitter
  records primary-then-secondaries in order. The plain `repeated`/`repeated_while` delimited
  drivers already ordered the close-status diagnostic before their bound checks.

- **The delimited many-builders commit the closer without re-lexing it, fixing a
  blackhole-cache (`ParserContext<_, _, ()>`) double-scan on the success path.** Internal,
  non-breaking. `InputRef::probe_close` used to classify the closer by scanning it and then
  push the scanned token back to the cache for a follow-up `try_expect` to commit; under the
  blackhole cache `()` the push-back is a no-op, so the closer was dropped and the follow-up
  `try_expect` **re-scanned** it. That second scan is observable to a stateful or
  resource-limited lexer — a valid delimited list (e.g. `(a)`) could trip its limiter, or hit
  the "unreachable" recovery path, on otherwise-valid input. `probe_close` now classifies the
  closer without consuming it — carrying the scanned token out by value, or leaving a cached
  closer at the front — and a new by-value commit primitive advances the cursor over it once, at
  the driver's own commit point, with zero re-scans in every cache capacity. Because the probe
  stays cursor-neutral until that commit, the deferred (`separated`/`separated_while`) drivers
  span their elements correctly and an error before the commit leaves the closer available for
  recovery. All four delimited many-builders (`repeated`, `repeated_while`, `separated_by_*`,
  `separated_by_*_while`) adopt it; the `DefaultCache` path is unchanged (it already scanned the
  closer exactly once). This also removes the same latent double-scan from the `Unclosed` fix
  above, which shipped the identical push-back pattern.

- **Unterminated delimited shape parsers now report the opener as `Unclosed` through the
  emitter, matching the many-builders.** The delimited shape parsers (`delimited::<D>`,
  `parens`, `braces`, `brackets`, `angles`, and their `try_` twins) used to raise a plain
  unexpected-token / end-of-input error when the closing delimiter never arrived; they now
  follow the same four-way close-miss law as the delimited many-builders above. End of input
  with the opener still open emits an `Unclosed` diagnostic carrying the **opener's span** and
  the delimiter pair's name (a fail-fast `Fatal` emitter fails with it; a recovering `Verbose`
  emitter records it and recovers, yielding the construct with a closer synthesized at the
  insertion point — a zero-width span); a wrong token where the closer belongs stays the
  existing unexpected-token (expected-close) diagnostic, not `Unclosed`; and a terminal scanner
  stop surfaces the committed form's end-of-input error, adding no `Unclosed`. The `try_` twins
  keep their decline law (absent opener ⇒ `Ok(None)`, zero consumption) and, once the opener is
  committed, report `Unclosed` on an unterminated group — never a silent decline.

### Changed (breaking)

- Added `UnclosedEmitter`, a new atomically-composable emitter sub-trait
  (`tokora::emitter::UnclosedEmitter`) with a single `emit_unclosed` method, implemented by the
  built-in `Fatal`, `Verbose`, `Silent`, and `Ignored` emitters.
- The delimited many-builder `ParseInput`/`Collect` implementations gained two bounds:
  - `Ctx::Emitter: UnclosedEmitter<'inp, L, Lang>` — **a custom emitter must now implement
    `UnclosedEmitter`** to be usable with `.delimited::<D>().collect()`;
  - `<Ctx::Emitter as Emitter<…>>::Error: From<Unclosed<(), L::Span, Lang>>` — **an error type
    used with a delimited many-builder must gain a `From<Unclosed<…>>` arm.**

  Both are source-breaking for consumers whose emitter or error types do not already satisfy the
  new bounds, hence the 0.4.0 (breaking) classification. The delimiter identity travels in the
  `Unclosed`'s name (`CowStr`); the type-level delimiter tag is the erased `()` (the builder
  reborrows the delimiter internally, so a `Delim`-parameterized bound would not unify across the
  builder's own indirection).
- The delimited **shape** parsers (`delimited`/`parens`/`braces`/`brackets`/`angles` and their
  `try_` twins) gained the same two bounds as the many-builders — `Ctx::Emitter:
  UnclosedEmitter<'inp, L, Lang>` and `<Ctx::Emitter as Emitter<…>>::Error: From<Unclosed<(),
  L::Span, Lang>>`. Source-breaking for shape-parser consumers whose emitter or error types do
  not already satisfy them, on the same footing as the many-builder change above.

### Migration

- Add a `From<Unclosed<…>>` arm to any error type used with `.delimited::<D>().collect()`, e.g.
  `impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for MyError { … }`. See the `json`
  example's `JsonError::Unclosed` arm for a worked pattern.
- If you use a custom emitter (not `Fatal`/`Verbose`/`Silent`/`Ignored`), implement
  `UnclosedEmitter` for it, mirroring your `FullContainerEmitter` impl: a fail-fast emitter
  converts the `Unclosed` to `Err` via `From`; a recovering emitter records it on its diagnostic
  log and returns `Ok(())`; a dropping emitter returns `Ok(())`.
- The same two migration steps apply to the delimited **shape** parsers (`delimited`/`parens`/
  `braces`/`brackets`/`angles` and their `try_` twins): add a `From<Unclosed<…>>` arm to the
  error type, and implement `UnclosedEmitter` for a custom emitter.
