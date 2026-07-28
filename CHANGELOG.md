# Changelog

All notable changes to this crate are documented here. The project follows semantic
versioning; before 1.0, a minor bump (0.x → 0.(x+1)) signals a breaking change.

## Unreleased

### Changed (breaking)

The Pratt driver ends an expression when the RHS channel says so, not when the input
position looks finished — and a report that consumes nothing can no longer be folded.

1. **`PrattRHS` gains an `End` variant.** A grammar says the expression is over on the
   same channel it uses to report operators, instead of relying on a sentinel operator
   below the minimum power. Exhaustive matches on `PrattRHS` must add an arm; the
   compile error is deliberate, which is why the enum is not `#[non_exhaustive]` — a
   wildcard arm is exactly the silent end-swallowing this change removes.

2. **The `is_eoi` loop gates are gone.** They ended the expression on a *position*,
   which a lookahead could move: a widening peek made a typed parse of `1+2` return
   `1`. The RHS channel now decides.

3. **Recursion floors are explicit.** `PrattFloor` replaces the arithmetic that
   reconstructed a floor from a power. Right-associative operators recurse at their own
   power (an off-by-one that made a right-associative pin yield 14 where 10 is correct);
   left and non-associative recurse strictly above it, which also removes the wrong
   parses the old code produced at `Power::MAX`, where the arithmetic saturated instead
   of separating. The token flavour carries its true left power rather than
   reconstructing it.

4. **`PrattPower::next` and `prev` are removed.** They were the arithmetic that made
   the floor bugs expressible; with them gone, reintroducing driver-side power
   arithmetic is a compile error rather than a review comment. This breaks callers, not
   only implementors, and no lint would have flagged them as unused — a required trait
   method warns nowhere. Note also that every in-repo implementation was non-saturating
   despite the trait's own documentation asking for saturation, so each carried a latent
   debug overflow panic.

5. **A report that consumes nothing is refused, terminally.** The typed driver checks
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

6. **The trace channel is more verbose.** With the position gate gone, the RHS channel
   is consulted at exhaustion, so traces gain the consultations the gate used to skip.
   Parse results and non-trace output are unchanged.

The token driver needs none of this: `PrattToken::try_pratt_rhs` and `try_pratt_lhs`
take only `&self` with no `InputRef`, so a report there cannot be made without
consuming, and no token fold receives an `InputRef`. That asymmetry is why the guard
lives in one driver and not the other.

### Changed (breaking)

The bound a grammar production writes is now **one line**. Where a production previously
restated every conversion its callees might need, it names a single context bundle and the
compiler elaborates the rest.

1. **`ParseCtx` is renamed `ComposableParseContext`** and completed. It now carries
   `FromTokenErrors` — the five token-level conversions as one supertrait — so the nested
   associated-type bound elaborates to callers. `ParseContext` is unchanged. The rename
   touches 59 occurrences across 14 files; the crate-root re-export set gained and lost
   nothing.

2. **`FromUnclosed` replaces the per-delimiter `From<Unclosed<D, …>>` family.** One bound
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

3. **The `_of` language twins are gone — 167 public items removed.** Every `X::parse_of` is
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

4. **The fluent `separated_by_*` family works for branded grammars.** It previously failed on
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

5. **`Token::Kind: 'static` is hoisted to the trait**, deleting five projection restatements.
   Four superficially similar bounds remain: they constrain the dispatch structs' own free
   `Kind` parameter, which the hoist does not reach.

6. **`Parser` loses its `Error` phantom parameter** and gains value-driven entry points
   (`parse`, `parse_with`, `parse_with_state`); its constructors collapse from eight to four.

7. **The delimiter side gets item 4's fix.** `Delimiter` and `TypedDelimiter`'s blanket impls
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

### Added

- **`Dialect`** — a two-item anchor (`Lang`, `Lexer`) with the projection aliases `LexerOf`,
  `LangOf`, `DialectSlice`, `DialectInput` and `DialectErrorOf`. Everything else a production
  needs — source, slice, token, span, offset, token capabilities — is reachable by projection,
  so pin those in a one-line subtrait rather than as further associated items.

  There is deliberately no `DialectCtx`. A context bound written through `LangOf<'inp, D>`
  does not unify with one written at the brand: the param-env predicate stays at the
  projection while the obligation normalises. This is documented at the type with the
  mechanism and the verbatim failure, so the shape is not rediscovered by trial.

- **`#[diagnostic::on_unimplemented]` on `FromTokenErrors`, `FromUnclosed` and `Dialect`.**
  A missing conversion now reports what is missing and carries a copyable impl skeleton,
  rather than surfacing as a nested obligation inside crate internals.

  Note for consumers: rustc attaches the *root* obligation's attribute. A bound reached
  through your own blanket-implemented subtrait reports that subtrait, not ours — annotate
  your own dialect traits if you want curated text there.

### Notes

`#[diagnostic::do_not_recommend]` was evaluated and rejected. It does not change whether the
curated message appears; its only effect is to hide *which* member is missing, which is the
one datum a consumer needs. Measured on 1.87 and on nightly.

### Added

- **`InputRef::is_exhausted()`** — the consumer-exhaustion predicate: no lexed token is waiting and
  the lexer frontier has reached the end of the buffer. This is the correct gate for a driver loop,
  where `is_eoi()` is not: `is_eoi` answers `true` the moment any lookahead lexes through the end,
  while the tokens that lookahead produced are still unconsumed. `is_exhausted()` is independent of
  the cache implementation.
- **`Cache::RETAINS_FRONT`** — a new trait constant declaring that a front push into an **empty**
  cache always succeeds, i.e. the cache can retain at least one token. It defaults to `false`, so
  the addition is semver-compatible and an existing implementation keeps working unchanged. A cache
  that declares `true` lets the input layer prove its parked-front slot statically unreachable, so
  every probe of it folds away at monomorphization; the shipped caches declare it accordingly
  (`Option` `true`, `GenericArrayDeque<_, N>` `N != 0`, the black holes `false`). A cache that
  declares `true` and then refuses a front push into an empty cache is violating the contract, and
  now panics at the refusal rather than losing the token.

### Fixed

- **A token left unconsumed by a `to`-shaped scan stop or by a `try_expect*` / `probe_close`
  decline is now retained even when the configured `Cache` refuses it**, so the cursor, offset,
  `is_eoi`, the `sync_balanced` hole anchor and the lexer-state tally after such a stop are the
  same under every cache capacity — including a zero-capacity `()`, where the token used to be
  dropped and re-lexed by the next call. `ClosePayload::CacheFront` is renamed
  `ClosePayload::AtFront` (crate-internal).
- **`InputRef::lexer()` now resumes under the state that produced the newest retained token**
  rather than under the committed state, so a widening lookahead no longer lexes the token after
  a retained run as if that run had never been scanned. A by-value `Lexer::State` limiter is no
  longer bypassed by the lookahead pattern, and the tokens a drain yields, the final state and
  the diagnostics it emits no longer depend on how deep — or in how many steps — the caller
  peeked first.
- **A non-final EOF now reports the lexer's own end offset** instead of the end of the last item
  it yielded, so bytes the lexer skipped before exhaustion (trailing whitespace, a comment tail)
  are no longer omitted from the `Incomplete` frontier a refill driver reads. The reported offset
  is floored by what was already lexed and clamped to the buffer.

### Fixed (behaviour-breaking)

Six deliberate behaviour changes, all in the repetition drivers' end-state accounting. No
API is removed or renamed.

1. **A properly closed separated + delimited list now reports its count bounds and separator
   policy.** `…separated_by_*().delimited::<D>()` left its element loop through the mid-scan
   closer without running the end-state pass, so on every well-formed, properly closed list the
   whole of `at_least` / `at_most` / `bounded` and the leading/trailing separator policy was
   dead code — the sibling `separated_*_while` driver ran it on the identical path. Under a
   recovering emitter such a list now records the diagnostic it always should have; under a
   fail-fast emitter a bounds- or policy-violating closed list is now an `Err` where it used to
   be a silent `Ok`.
2. **`TooMany` is emitted once per construct, with a count that exceeds the limit it names.**
   `nums()` is now `limit() + 1` at every emission site. It used to be either the running count
   (so the same four-element history yielded a different payload from each builder) or, from
   the mid-loop hook, the limit itself — a "found 2 … exceeds … 2" that contradicted itself.
   The duplicate emission (mid-loop hook plus a delegating end check) is gone.
3. **`FullContainer` is emitted once per construct**, on the whole-construct span in every
   driver, with a `nums()` that includes the refused element. A container that refuses one push
   refuses every later one, so the old per-dropped-element re-emission produced a count that
   climbed past the capacity it named.
4. **Count bounds now judge the elements the driver parsed, not the ones the container
   stored.** In the separated drivers a bounded container used to turn a satisfied `at_least`
   into a spurious `TooFew`, and — worse — to swallow a violated `at_most` entirely by clamping
   the count the check reads. Only bounded-capacity containers are affected; with `Vec`,
   `VecDeque`, `SmallVec` and `TinyVec` the stored and parsed counts are always equal.
5. **`SeparatorHandler::on_separator` now receives every separator in source order.** It used
   to receive the exact complement of its documented contract — only the leading separator and
   the duplicates in a run, never the happy-path or trailing ones. A container that ignores
   separators can opt out with the new defaulted `SeparatorHandler::OBSERVES_SEPARATORS`
   associated const, which suppresses both the call and the clone that feeds it; every
   container in this crate does. Adding the const makes `SeparatorHandler` no longer
   object-safe — `dyn SeparatorHandler` no longer compiles. Nothing in this crate used it.
6. **The delimited separated drivers return the whole construct's span**, opener through
   closer, on every success exit. Some exits previously returned a span measured from the first
   element instead, so the returned span excluded the opener and depended on which exit the
   parse took.

### Fixed

- **`InputRef::foldr_within` no longer drops a run shorter than its window.** The exhaustion
  arm returned the untouched initializer *after* consuming the run into the fold's buffer, so
  every run of length `1 ..= W::CAPACITY - 1` was silently discarded on the `Ok` path. Both
  exits now converge on the single reverse drain, as the sibling `foldrn` already did.

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
