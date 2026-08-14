//! Conformance test kit for custom [`Cache`] implementations.
//!
//! # Why this exists
//!
//! `Cache::rewind` used to be part of this trait, and it could not be implemented correctly from
//! what it was given: the method received a `&Checkpoint`, whose public surface is the cursor and
//! the state, while the datum its own documentation named — which entries were pushed after the
//! save — is crate-private. A third-party implementation had no way to be right, and one that
//! tried over-dropped pre-save lookahead: the region then re-lexes, so limit budgets are
//! re-burnt, instrumentation doubles, and poison can latch at a position the original lineage
//! never reached.
//!
//! So the method is gone and the geometry moved into the input layer, which has both halves of
//! the facts. What is left for a cache to uphold is a **queue contract** — and a contract that
//! only exists as prose is a contract nobody can check. This kit is its executable form: build a
//! [`CacheHarness`] over a source your lexer can read and call [`run`](CacheHarness::run). It
//! panics, with precise context, on the first violation.
//!
//! # What it checks
//!
//! The kit's fundamental observable is the **cached entry** — the triple
//! (`L::Span`, `L::Token`, `L::State`) that a [`CachedToken`] carries. Wherever a check reads an
//! entry back out of a cache — through `front`, `back`, `pop_front`, `pop_back`, `peek`,
//! `peek_one`, the token a refused push hands back, `push_many`'s overflow, or the entry a
//! `pop_front_if` predicate is given — all three components are compared, componentwise, against
//! what the kit stored at that position, each with its own message.
//!
//! The `L::State` half is the one with teeth. It is what the input layer restores a lexer
//! **from**: `InputRef::lexer` rebuilds the lexer at the lookahead frontier out of the state the
//! newest retained entry carries ([`crate::Lexer::with_state`] + [`crate::Lexer::bump`]), and
//! the rollback path reads the same field off the entries
//! `pop_back` hands out. A cache that returns the right spans beside the wrong states does not
//! merely fail to be certified: until #183 it **passed**, in full, and then corrupted every
//! restore that resumed from it.
//!
//! Each check is one law from the [`Cache`] trait contract:
//!
//! 1. **Empty invariants** — a fresh cache is empty on every observable at once: `len` 0,
//!    `is_empty`, no `front`/`back`, nothing to pop, no `span`, an empty `peek`.
//! 2. **`RETAINS_FRONT` honesty** — a cache that declares `true` must accept a `push_front` into
//!    an empty cache: both into a *fresh* one, where that push is the first operation the cache
//!    ever sees, and into one that has been used and emptied. (The input layer compiles its
//!    parked-slot fallback *out* on that declaration, so a cache that declares it and then
//!    refuses is losing tokens.) The *law* underneath — an empty cache with capacity accepts a
//!    front push, because a push is refused only by a full one — belongs to check 5 and is driven
//!    for **every** nonzero capacity whatever the declaration says. What this check adds is what
//!    the declaration costs.
//! 3. **FIFO append and exact length** — `push_back` places each token after every resident one;
//!    `len` is exactly the resident count and `remaining` exactly how many more `push_back`s will
//!    be accepted, at every step; and a refused push **returns the token unchanged** and leaves
//!    every observable untouched. `is_empty` is checked against the same residency here too, not
//!    only at check 1's fresh cache: `is_empty` is a *default* method (`len() == 0`) a fixture
//!    can override, and check 1 alone only ever calls it where `len` has already established the
//!    answer is `true` — a constant-`true` override was otherwise indistinguishable from a
//!    correct one at every OTHER residency this kit drives.
//! 4. **Order** — `pop_front` removes the oldest and `pop_back` the newest, and `front`/`back`
//!    view those same two entries without removing them. The input layer's restore path is built
//!    on the `pop_back` half. `front`/`back` are read **themselves**, at every residency the kit
//!    builds, and not only through `front_span`/`back_span`: those two are *default* methods
//!    derived from these, but a cache is free to specialize them off a head and tail index, and
//!    then the entry a caller reads — the one carrying the token and the `L::State` a restore
//!    resumes from — can name a different slot than the span accessor does.
//! 5. **`push_front` prepends** — before every resident entry, with the same refusal
//!    round-trip, and the same requirement that a refusal be **warranted**: a push is refused
//!    only by a full cache, on this arm exactly as on `push_back`'s. The prepend order is
//!    checked in **full**, not at its two ends: the resident sequence is drained after each
//!    accepted prefix — from the front, and then from the back, since every other `pop_back` the
//!    kit drives runs against a cache `push_back` filled from empty, and a tail index a prepend
//!    invalidated is wrong only on a residency a prepend built — because a `push_front` that
//!    lands its token at the front and permutes what
//!    is behind it has the right front, the right back and the right length. The two arms are
//!    also **interleaved**, not run one after the other: a mixed residency built as one
//!    `push_back` and then every `push_front` never once puts an append after a prepend, so a
//!    cache whose append computes its slot from a head index a prepend has since moved is
//!    correct in every sequence the rest of the kit builds. And the refusal law
//!    is driven at the **empty** cache too, at every nonzero capacity — the one state the
//!    prepend driver cannot reach, because it has to seed the cache with a `push_back` before
//!    there is anything to prepend before. And it is driven at **capacity 1**, where the seeding
//!    push fills the cache and so every front push after it is refused: the prepend order is not
//!    observable at that capacity, but the refusal is the only thing observable there, and no
//!    other check reaches a refused push on this arm — check 3 drives the round-trip for
//!    `push_back`, and the two accepted-front-push checks never see one refused.
//! 6. **Bounded, pure `peek`** — appends exactly `min(len, the buffer's REMAINING capacity at
//!    call time)` entries, oldest first, each resident token once, leaving whatever the buffer
//!    already held untouched; changes no observable; and answers identically when called twice
//!    on an unchanged cache. Driven against an empty buffer *and* against a prefilled one — both
//!    are real. Remaining and total capacity are the same number only while the buffer is empty,
//!    which is what the real call site hands over on a cache hit or an overflow-free fill;
//!    otherwise it hands over a prefilled one, already holding the parked token or staged
//!    overflow. The prefilled driver sweeps **every prefix
//!    depth** the window can hold, from one entry to a buffer with no room left, rather than
//!    fixing one: at a single depth a `peek` that is correct behind that prefix and wrong behind
//!    a deeper one is indistinguishable from a conforming one. The whole of it is driven at
//!    **every residency**, from a full cache down to one drained empty, since at
//!    `len == capacity` a `peek` bounded by the backing store and a `peek` bounded by the live
//!    run are the same number — and each residency is reached from **both ends**, by
//!    `pop_front` and by `pop_back`, since removed entries a cache fails to clear sit ahead of
//!    the live run on the first path and behind it on the second, and only the sweep that drains
//!    that end reads them. `pop_back` is the input layer's restore path. The whole of it is then
//!    driven once more against **one instance**, peeked in full and then popped and re-peeked at
//!    every residency down to empty — because a sweep that builds a fresh cache per residency
//!    only ever asks a cache its FIRST question, and a `peek` that memoises its first answer and
//!    serves it after later pops is pure, is correct at the residency it latched, and is
//!    otherwise invisible. Those pops are the only ones in the kit that run on an already-peeked
//!    instance, so the **entry** each of them returns is compared there too: a `peek` that leaves
//!    a cached lookahead frontier behind and lets a later pop report its `L::State` out of that
//!    instead of out of the entry it removed keeps every span and every residency correct, and
//!    hands the caller a lexer position no entry ever had. And the bound and order are read through **three window types**, not
//!    one: `peek` is generic over `W` and may read `W::CAPACITY` off it, so a single fixed window
//!    leaves a branch on that parameter unreachable — the single-slot fast path the trait invites
//!    (`peek_one`'s default body is `peek::<U1>`), and the truncating path a cache takes when the
//!    residency does not fit in the window, which never runs while the window is as wide as every
//!    residency driven. Sweeping the prefill depth does not stand in for this: that varies the
//!    room LEFT in the buffer, not the type. The prefilled half is also driven with the buffer's
//!    entries **before** the residency in source order, not only after it: that relation was one
//!    fixed value in every prefilled call the kit made, and the value it was fixed at is the
//!    inverse of the real one — the peek fill pushes the parked token first, because the parked
//!    token is the front of the stream. Both are now driven, and they catch different things: a
//!    `peek` that sorts or merges the buffer by span is caught by the *later* arrangement, where
//!    a correct append leaves the buffer unsorted, and a `peek` that reads the buffer as a prefix
//!    of its own run and skips an entry it decides the caller already holds can only act in the
//!    *earlier* one. `peek_one` agrees with
//!    `front`, answers nothing where there is no front, and — like `peek` — answers **the same
//!    on a second call** against an unchanged cache: it takes `&self`, so the two calls are the
//!    same question.
//! 7. **`clear`** — returns the cache to the check-1 state, AND the cache stays usable
//!    afterward: it is refilled with `cap` fresh pushes and checked against check 3's oracle,
//!    so a `clear` that empties the cache but also permanently disables it (poisons it, zeroes
//!    its capacity) does not pass by never being asked to accept another token.
//! 8. **`span`** — the combined span runs from the front entry's start to the back entry's end,
//!    swept at every residency check 6 sweeps (full, and partially drained from both ends), not
//!    checked once against a freshly filled cache: a `span` that is correct immediately after a
//!    fill and stale after any pop is indistinguishable from a conforming one at the one
//!    residency a single check reaches.
//!
//!    Checks 6 and 8 are then both re-run against a **rotated** residency — drained off the front
//!    and topped back up to capacity — because those two are the operations that WALK from the
//!    head, and a ring's live run only ever wraps past the end of its backing array when
//!    something is pushed *after* something was popped. Every other residency the kit builds
//!    starts from an empty cache and only shrinks, so the classic missing `% capacity` truncates
//!    by zero at all of them. `front`, `back` and the pops name a single slot rather than walking
//!    to one, and an index that names the wrong slot is already caught by the residency oracle.
//! 9. **`pop_front_if` / `try_pop_front_if`** — the predicate sees the front entry and nothing
//!    else; a `false` (or `Err`) answer removes nothing and leaves every observable untouched; a
//!    `true` (or `Ok(())`) answer removes exactly the front entry, matching what a bare
//!    `pop_front` would return from an identically filled cache. Against an empty cache, both
//!    answer `None` and the predicate never runs at all — there is no front to hand it. The
//!    predicate's **argument** is recorded and compared at **every arm that runs it** — both
//!    methods, and the DECLINING answer as well as the accepting one. It took two findings to get
//!    there. The two `try_pop_front_if` closures used to throw the argument away, so everything
//!    the check asserted about that method was its return value and its residency. Then the
//!    `pop_front_if` arm driven with a `false` predicate turned out to do the same, because a
//!    site that answers `None` had been filed under "absence is the law" — a classification made
//!    on what the call RETURNS, when what makes these two methods different is that they hand an
//!    entry to caller code BEFORE they know what to return. Either way an override that ran the
//!    caller's validation predicate against `back()` and then produced the conforming return and
//!    the conforming residency satisfied all of it. That is a cache whose caller-supplied
//!    predicate decides whether to remove or retain the front on the strength of unrelated
//!    lookahead, certified by a check whose own prose named the property it was not testing.
//! 10. **`push_many`** — accepts tokens in order until the cache is full, then hands the
//!     remainder back through the returned iterator, unchanged and in the order they arrived.
//!
//! The kit adapts to capacity: it runs every check a cache of that capacity can express, so the
//! capacity-0 `()` cache, the capacity-1 `Option` cache and an arbitrarily wide ring are all
//! driven by the same call.
//!
//! **Both constructors.** `Cache` has two — [`new`](Cache::new) and
//! [`with_options`](Cache::with_options) — and the kit builds with the first. Hand it
//! [`also_built_by`](CacheHarness::also_built_by) and it runs the whole contract a second time
//! against caches the second one builds, re-reading the capacity from each pass so the two may
//! differ, and tagging every message with which constructor it is talking about. It cannot reach
//! `with_options` on its own: [`Options`](Cache::Options) is an associated type the kit has no
//! way to fabricate a value of.
//!
//! # What it deliberately does not check
//!
//! **Whether `with_options` honours the options it was given.** The pass above certifies that a
//! `with_options`-built cache obeys the whole contract, which is what catches the shapes that
//! matter — one that comes back non-empty, or that reports a capacity it will not honour. What
//! no oracle generic over `L` and `C` can check is the *relation* between the options and the
//! cache: `Options` is opaque, so a `with_options(16)` that hands back a capacity-3 cache is
//! internally consistent, and internally consistent is all the kit can see. That relation is the
//! implementor's to test, in a test that knows what the options mean.
//!
//! **The zero-re-scan law** (a probed closer is committed exactly once, cache-independently, in
//! any capacity) is an *input-layer* law, not a queue law: its oracle drives `probe_close` and
//! `commit_probed`, which are crate-internal, and reaching it through public API needs a
//! delimited grammar that a kit generic over `L` cannot synthesize. It is covered in-tree
//! instead, at the mechanism level and through the `separated` driver, over capacities 0, 1, 3
//! and 8 — see `commit_probed_lexes_the_closer_once_under_every_cache` and
//! `tests/probe_close_no_rescan.rs`. Said here rather than implied by omission.
//!
//! **A `peek` bounded by the buffer's TOTAL capacity, when it discards the overflow in
//! silence.** Check 6 states the bound as the buffer's *remaining* capacity and drives it with a
//! prefilled buffer, which is the only shape where remaining and total differ at all — but a
//! cache that reads the wrong one and then simply ignores what `push_back` hands back is
//! **provably invisible** to this kit, and to any caller of the `Cache` trait.
//!
//! Two facts make it so. `GenericArrayDeque::push_back` on a full deque returns the value and
//! leaves the deque *unmodified*, so an over-count leaves no trace in the buffer; and the
//! arithmetic collapses — with `W` the buffer's capacity, `P` what it already holds and
//! `R = W - P` the room left, a `peek` that pushes the oldest `min(len, W)` entries in order
//! lands `min(min(len, W), R) = min(len, R)` of them, which is the correct count, in the correct
//! order, for **every** `len`, `W` and `P`. No window size, prefill depth or cache capacity the
//! kit could choose separates the two, `peek` takes `&self` so there is no residency to differ,
//! and the surplus entries are dropped inside the cache where the kit has no hook. There is
//! nothing left to assert on.
//!
//! So the kit does not claim to catch it. What it does claim, and checks, is the observable half
//! of the same law: a `peek` that **writes over** the entries the buffer already held, rather
//! than appending behind them, fails check 6 — that one has a consequence, and the prefilled
//! driver is what exposes it. `cache_tests.rs` pins both halves: one fixture that the kit
//! catches, and one silent total-capacity fixture that the kit is asserted to **accept**, so
//! this paragraph stops being true the moment that assertion starts failing.
//!
//! **What an empty cache's `pop_front_if`/`try_pop_front_if` predicate is SHOWN, if it wrongly
//! runs at all.** These two methods are the only ones on `Cache` that hand an entry to caller
//! code, and they do it before they know what to return — so for them a `None` return is not
//! evidence that nothing was handed over. Every arm the kit drives against a *non-empty* cache
//! therefore records the predicate's argument and compares it, the declining arm included. The
//! two empty-cache probes do not: their law is that the predicate must **never run**, which is
//! asserted directly, and a cache that runs one has already failed on the louder violation. What
//! the kit does not then say is which entry it conjured to run it with. No cache is certified
//! because of this — reaching the case requires failing the check — and it is written down here
//! rather than left implied, because the round that closed the declining arm found it by
//! noticing that exactly this sentence had never been written for it.
//!
//! **The non-panicking restore path** cannot be checked by running code: the input layer calls
//! `pop_front`, `pop_back`, `clear`, `front`, `front_span` and `len` from guard drops that may
//! run mid-unwind, where a panic aborts the process. A test cannot survive its own witness. The
//! law is on the trait; this kit exercises those six operations so a panicking one fails *here*,
//! in a test, rather than in a consumer's rollback.
//!
//! **A permutation of entries that are equal to begin with.** Every oracle compares the whole
//! entry — span, token and state — so a *substitution*, an entry that differs from the one the
//! kit stored at that position, is always caught. What no comparison can see is a permutation of
//! values that were already indistinguishable. The kit's discrimination is therefore exactly the
//! **pairwise distinctness of the tokens and the states your corpus carries**.
//!
//! Spans are pairwise distinct from every source, because `corpus` lexes successive items and
//! successive items occupy successive positions — so the ordering laws never lose discrimination,
//! whatever the kit is pointed at. Tokens and states are not free that way. A corpus of one
//! fieldless token variant has one inhabitant, so a token comparison over it is vacuous; a lexer
//! whose `L::State` is `()` — which is every logos `Extras` that was not given one — makes the
//! state comparison vacuous in the same way.
//!
//! A constant state is not a gap for **that** lexer: a single-valued state has nothing to be
//! re-associated with, so a cache cannot corrupt it. It is a gap for a cache **intended for
//! stateful lexers** and certified under a stateless one, which is the one combination this
//! paragraph exists to warn about. Certify such a cache under a corpus whose tokens and states
//! are pairwise distinct — see [`new`](CacheHarness::new) — and the entry comparison discriminates
//! every re-association a cache can make.
//!
//! # Violation posture
//!
//! A failing check is a bug in the *cache* (or a mismatch with the documented contract),
//! surfaced loudly. The kit never mutates behaviour; it observes and asserts.

use std::{format, vec::Vec};

use core::{cell::Cell, marker::PhantomData};

use generic_arraydeque::{
  GenericArrayDeque,
  typenum::{U1, U3, U4, Unsigned},
};
use mayber::Maybe;

use crate::{
  Lexer, Slice, Source, Span, Window,
  cache::{
    Cache, CachedToken, CachedTokenOf, CachedTokenRefOf, MaybeRefCachedTokenOf, PeekedTokenExt,
  },
  span::Spanned,
};

/// One peeked entry, flattened out of [`Maybe`]'s two arms into the triple the kit compares.
///
/// `peek` may hand back a borrow of a resident entry or an owned clone — [`Cache::peek`]'s own
/// contract allows either — and the kit reads both arms the same way: the span and the token
/// through [`PeekedTokenExt`], the state through [`peeked_state`]. Cloning the three components
/// out is also what makes the purity law expressible, since [`CachedToken`] has no `PartialEq`
/// (the kit compares componentwise, for the message quality that buys) and a `Maybe` holding a
/// borrow cannot outlive the `peek` call that produced it.
type PeekedTriple<'inp, L> = (
  <L as Lexer<'inp>>::Span,
  <L as Lexer<'inp>>::Token,
  <L as Lexer<'inp>>::State,
);

/// The peek window the kit peeks through. Four is enough to distinguish "bounded by the buffer"
/// from "bounded by the residency" on both sides of the default cache capacity, and to give the
/// prefill sweep four distinct shapes to hand `peek`: a prefix of one, two intermediate depths
/// with an order of their own and room still behind them, and a buffer with no room left. It is
/// also what every source the kit is pointed at has to carry past the cache's capacity, so it
/// buys those shapes at four tokens rather than at a longer corpus.
type PeekWindow = U4;

/// The single-slot window, driven beside [`PeekWindow`] so that `W` is not one fixed type in
/// every driver the kit has (#180 part B, item 4).
///
/// This one is not an arbitrary second value: [`Cache::peek_one`]'s default body is
/// `peek::<U1>` into a one-slot buffer, so a one-slot fast path is a specialization the trait
/// itself invites — and an implementation that overrides `peek_one` (the hot path) while getting
/// `peek::<U1>` wrong has two answers to the same question, only one of which the kit was asking.
type PeekWindowOne = U1;

/// A window strictly between [`PeekWindowOne`] and [`PeekWindow`], so that at the residencies
/// this kit builds there is a call where the window is **narrower than the residency** and the
/// bound genuinely truncates.
///
/// At `PeekWindow` that never happens for a cache of capacity 4 or less, and at `PeekWindowOne`
/// the truncated run is a single entry, which has no order to get wrong. So a cache with a
/// separate code path for "the residency does not fit" — the branch where a copy has to stop
/// short of `back`, and the one place `peek` can walk in the wrong direction — is reached here
/// and nowhere else.
type PeekWindowMid = U3;

/// Where the entries a prefilled `peek` finds already in the buffer sit **relative to the
/// residency**, in source order.
///
/// Until #180 part B, item 6 this was one fixed value: every prefill the kit built came from past
/// the residency, so the buffer's spans were always greater than every resident one, and a `peek`
/// that reasons about the two — sorting, merging, or deciding the caller already holds its own
/// front — was answering the same question in every call the kit made. Both relations are now
/// driven, and the one the real call site has is [`Earlier`](Prefill::Earlier).
#[derive(Clone, Copy)]
enum Prefill {
  /// Corpus tokens **past** the residency. The kit's original prefill, and the one that keeps
  /// "`peek` left the buffer alone" honest at the residencies the sweeps build: a `peek` that
  /// clears the buffer and re-appends its own run cannot land the prefill's spans by coincidence,
  /// because the cache's front is a token the prefill does not contain.
  Later,
  /// Corpus tokens **before** the residency, which is the relation `InputRef`'s peek fill
  /// actually produces: the parked token is the front of the stream, so it heads the window and
  /// the cache fills in behind it. A cache that reads the buffer as a prefix of its own run —
  /// and skips an entry it decides the caller already has — can only do so in this arrangement.
  Earlier,
}

impl Prefill {
  /// The message tag, so a failure says which of the two arrangements it is talking about.
  const fn tag(self) -> &'static str {
    match self {
      Self::Later => "prefilled",
      Self::Earlier => "prefilled-with-earlier-tokens",
    }
  }
}

/// The span of a cached token, owned. `token()` hands back a `Spanned<&T, &Span>`, so the span
/// arrives double-borrowed and has to be dereferenced once before it can be cloned.
///
/// Since #183 the kit's observable is the whole entry, not the span — see
/// [`assert_entry_eq`](CacheHarness::assert_entry_eq). This projection stays for the two jobs that
/// are genuinely span-valued: building the expectation lists the span-level laws read
/// ([`assert_span_at`](CacheHarness::assert_span_at)'s combined span, the `front_span`/`back_span`
/// accessors), and putting spans in failure messages, where they are the vocabulary a reader
/// locates an entry by.
fn span_of<'inp, L>(tok: &CachedTokenOf<'inp, L>) -> L::Span
where
  L: Lexer<'inp>,
{
  (*tok.token().span_ref()).clone()
}

/// The `L::State` a peeked entry carries, whichever arm of [`Maybe`] it arrived on.
///
/// [`PeekedTokenExt`] reaches the token and the span arm-blind and deliberately does not reach the
/// state: giving it a third accessor would need a third type parameter on that trait, a breaking
/// change to a released public surface for a convenience nothing on the restore path needs. So
/// the kit matches the arm itself. Third parties can read the same fact through public API — match
/// the [`Maybe`] and call [`CachedToken::state`] — so this checks a publicly observable value, not
/// a crate-private one.
fn peeked_state<'s, 'r, 'inp, L>(entry: &'s MaybeRefCachedTokenOf<'r, 'inp, L>) -> &'s L::State
where
  L: Lexer<'inp>,
  'r: 's,
{
  match entry {
    Maybe::Ref(borrowed) => borrowed.state,
    Maybe::Owned(owned) => &owned.state,
  }
}

/// The consequence sentence the `back()` and `pop_back` state comparisons carry, and nothing else
/// does.
///
/// Those are the two reads the input layer's **restore** path makes: `InputRef::resume` takes
/// `cache().back()`, clones its `L::State` and rebuilds the lexer with `Lexer::with_state` +
/// `bump`, and the rollback drops an abandoned continuation with a run of `pop_back` calls. A
/// wrong state at either of them is not a cosmetic divergence — it is a lexer rebuilt from a state
/// no position in the source ever had.
const RESTORE_NOTE: &str = " This entry's `L::State` is what `InputRef::lexer` restores the lexer from (`with_state` + `bump`), so a right span with a wrong state beside it passes nothing here: it corrupts every restore that resumes from it.";

/// No consequence sentence — every state comparison that is not on the restore path.
const NO_NOTE: &str = "";

/// A conformance harness that drives a [`Cache`] implementation `C` against the cache contract.
///
/// The corpus is lexed from a source with `L`, so the kit needs no way to fabricate tokens and
/// the tokens it pushes are real ones. Build one, then call [`run`](Self::run).
///
/// # Example
///
/// ```
/// # #[cfg(all(feature = "logos_0_16", feature = "std"))]
/// # fn demo() {
/// use tokora::cache::DefaultCache;
/// use tokora::conformance::cache::CacheHarness;
/// # type MyLexer<'a> = tokora::lexer::LogosLexer<'a, MyTok>;
/// # #[derive(Clone, Debug, PartialEq, tokora::logos::Logos)]
/// # #[logos(crate = tokora::logos, skip r"[ \t]+")]
/// # enum MyTok { #[regex(r"[a-z]+")] Word }
/// # impl core::fmt::Display for MyTok {
/// #   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str("word") }
/// # }
/// # #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] struct MyKind;
/// # impl core::fmt::Display for MyKind {
/// #   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str("word") }
/// # }
/// # impl tokora::Token<'_> for MyTok {
/// #   type Kind = MyKind; type Error = ();
/// #   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
/// #   fn kind(&self) -> MyKind { MyKind }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// // Seven tokens: `DefaultCache`'s capacity of 3, plus the 4-token peek window the kit
/// // prefills the peek buffer from.
/// CacheHarness::<MyLexer<'_>, DefaultCache<'_, MyLexer<'_>>>::new("a b c d e f g").run();
/// # }
/// ```
pub struct CacheHarness<'inp, L, C, Lang: ?Sized = ()>
where
  L: Lexer<'inp>,
{
  source: &'inp L::Source,
  name: &'static str,
  /// How the cache under test is built on THIS pass. `None` is `C::new()`;
  /// [`also_built_by`](CacheHarness::also_built_by) stores a second constructor here and
  /// [`run`](CacheHarness::run) makes a second pass with it.
  ///
  /// A plain `fn` pointer rather than a boxed closure, so this needs no allocation and — the
  /// reason it is a constructor for `C` and not a factory for `C::Options` — no `C::Options`
  /// bound on the struct, which would be a breaking change to a released public type. A closure
  /// that captures nothing coerces to one, so `|| C::with_options(..)` is what a caller writes.
  built_by: Option<fn() -> C>,
  /// The suffix every failure message on this pass carries after the cache's name, so a failure
  /// says which constructor built the cache it is talking about. Empty on the `C::new()` pass.
  via: &'static str,
  /// Raw lex attempts per source unit the corpus builder may make. See
  /// [`lex_attempts_multiple`](CacheHarness::lex_attempts_multiple).
  lex_multiple: usize,
  _cache: PhantomData<fn() -> C>,
  _lang: PhantomData<fn(&Lang)>,
}

/// The cache's name, plus which constructor built it on this pass, as one `Display` value.
///
/// `Copy`, so the closures inside the kit's `unwrap_or_else` panics can capture it exactly the
/// way they captured the bare `&'static str` before, and every `{name}` in every message
/// interpolates unchanged.
#[derive(Clone, Copy)]
struct Label {
  name: &'static str,
  via: &'static str,
}

impl core::fmt::Display for Label {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(self.name)?;
    f.write_str(self.via)
  }
}

impl<'inp, L, C, Lang> CacheHarness<'inp, L, C, Lang>
where
  L: Lexer<'inp>,
  L::State: Clone,
  // The kit's observable is the whole cached entry, so the token and the state have to be
  // comparable (#183). Both bounds sit HERE, on the harness, and not on `Lexer::Token` or on
  // `State`: a supertrait would tax every production implementor — including every logos
  // `Extras` — to serve a test kit. A kit user that hits them derives `PartialEq`, or certifies
  // against a discriminating toy lexer, which is sound because a cache is generic over the
  // entries it stores.
  L::Token: PartialEq,
  L::State: PartialEq,
  Lang: ?Sized,
  C: Cache<'inp, L, Lang>,
{
  /// Creates a harness over `source`, from which the kit lexes its token corpus.
  ///
  /// The source must yield the cache's capacity plus a full peek window of items — the residency
  /// the checks build, plus the tokens past it that check 6 prefills the peek buffer with — and
  /// at least two in any case, since a queue law about ordering is not observable on one element.
  /// `run` says so, with the number, if it does not.
  ///
  /// **Variety matters as much as length.** The kit compares whole entries — span, token and
  /// state — so what it can discriminate is bounded by how distinguishable the corpus is. A
  /// source whose tokens are all one fieldless variant has one token value in it, and a lexer
  /// whose `L::State` never changes has one state value in it; against either, that half of the
  /// comparison cannot fail. Spans are pairwise distinct from any source, so the ordering laws
  /// hold up regardless. Point the kit at a source whose tokens carry distinct payloads, under a
  /// lexer whose state advances as it lexes, and every re-association a cache can make is
  /// visible. The module docs' *what it deliberately does not check* section states the bound
  /// exactly.
  #[must_use]
  pub fn new(source: &'inp L::Source) -> Self {
    Self {
      source,
      name: "cache",
      built_by: None,
      via: "",
      lex_multiple: super::DEFAULT_BUDGET_MULTIPLE,
      _cache: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Names this cache in every failure message — useful when one `#[test]` runs the kit over
  /// several caches and a bare panic would not say which.
  #[must_use]
  pub fn named(mut self, name: &'static str) -> Self {
    self.name = name;
    self
  }

  /// Raises the corpus builder's anti-hang ceiling to `multiple * source_units + 64` raw lex
  /// attempts. The default is 8.
  ///
  /// # Why this is a knob and not a derivation
  ///
  /// The ceiling exists because the corpus builder fills a target count of *tokens* while
  /// its loop consumes the whole item stream, and an `Err` grows neither: a lexer that only errors
  /// reaches neither the target nor exhaustion. Something has to stop that, and only an attempt
  /// count can.
  ///
  /// But the default was justified by an invariant this kit does **not** enforce. "Monotone
  /// progress and nonempty spans bound a conforming lexer at one item per source unit" is not what
  /// the lexer contract says: starts must be *non-decreasing*, not strictly increasing, and spans
  /// must be individually nonempty, not disjoint. Overlapping items and repeated starts are legal,
  /// so a deterministic, finite, terminating lexer may emit many items per source unit — and one
  /// that emits more than `8 * units + 64` of them before the tokens the kit asked for was refused
  /// as nonterminating with no way to say otherwise.
  ///
  /// That is a false *failure* rather than a false pass — the guard rejects a legitimate lexer, it
  /// never accepts a spinning one — but a certification that cannot be run is not a safe failure.
  /// This is the way to say otherwise.
  ///
  /// `0` is treated as `1` and anything above `65536` **panics**, matching
  /// [`Harness::budget_multiple`](super::Harness::budget_multiple) in both directions.
  ///
  /// The cap is not tidiness. `usize::MAX` here made the ceiling `usize::MAX` too, and under the
  /// `attempts > limit` order of the day no counter could reach one past the largest value it can
  /// hold — so the refusal was unreachable and the knob *disarmed the guard it configures*: the
  /// endless lexer above ran until the process was killed. The comparison is now `attempts >=
  /// limit` asked before the increment, which such a ceiling does reach, after `usize::MAX`
  /// attempts — enforced, and still not a refusal anybody is present for. A ceiling has to be a
  /// number the count passes while the caller is still waiting.
  ///
  /// # Why above the cap is a refusal and not a clamp
  ///
  /// The clamp was silent, and it *lowered* the ceiling the caller asked for. Everything the
  /// paragraphs above say about the default — that a finite, terminating lexer may legitimately
  /// need more attempts than a per-unit multiple predicts, and that refusing it is a false failure
  /// with no way to say otherwise — applies again one level up, to the caller who says so and is
  /// quietly given a smaller number. They then read the kit's `lex-budget` refusal as a verdict on
  /// their lexer. Above 65536 attempts per source unit this kit cannot tell a dense lexer from one
  /// that never reaches the target, and the honest answer is to say that at the call site.
  ///
  /// Below `1` the adjustment stays silent, because it only widens the ceiling and a wider ceiling
  /// cannot manufacture a refusal.
  ///
  /// It is spelled `lex_attempts_multiple` and not `budget_multiple` because it scales a different
  /// quantity: the lexer harness's budget bounds the *items a run may produce*, this bounds the
  /// *attempts the corpus builder may make*.
  ///
  /// # Panics
  ///
  /// Panics when `multiple` exceeds `65536`, naming this knob and that maximum. The panic comes
  /// from the builder, before any lexing, so it is never mistakeable for the corpus builder's
  /// `lex-budget` refusal.
  #[must_use]
  pub fn lex_attempts_multiple(mut self, multiple: usize) -> Self {
    let cap = super::MAX_BUDGET_MULTIPLE;
    assert!(
      multiple <= cap,
      "tokora cache conformance: CacheHarness::lex_attempts_multiple is capped at {cap} attempts \
       per source unit and was given {multiple}. The request is refused rather than lowered to \
       {cap}: a silent clamp configures a ceiling you did not ask for, and a lexer that \
       legitimately needs more attempts than the clamped ceiling allows is then refused as if it \
       never terminated — a failure that reads as the lexer's and is the kit's. Above {cap} per \
       unit this kit cannot tell a dense lexer from one that never reaches the target."
    );
    self.lex_multiple = multiple.max(1);
    self
  }

  /// Runs every check a **second** time against caches built by `make` instead of by
  /// [`Cache::new`], and is how [`Cache::with_options`] gets driven at all.
  ///
  /// Without this the kit builds every cache it tests with `C::new()`, so a conformant `new` and
  /// a broken `with_options` — one that hands back a cache with the wrong capacity, or one that
  /// is not actually empty — were indistinguishable: nothing ever called the second constructor
  /// (#180 part A, item 7). The kit cannot reach it on its own, because [`Cache::Options`] is an
  /// associated type it has no way to fabricate a value of.
  ///
  /// Pass the constructor, not the options:
  ///
  /// ```ignore
  /// CacheHarness::<MyLexer<'_>, MyCache<'_>>::new(SRC)
  ///   .also_built_by(|| MyCache::with_options(MyOptions { capacity: 16 }))
  ///   .run();
  /// ```
  ///
  /// Both passes run the whole contract, independently: the capacity is re-read from the cache
  /// each pass builds, so the two may differ, and the corpus must be long enough for the larger
  /// of them. Every failure message names which constructor built the cache it is about.
  #[must_use]
  pub fn also_built_by(mut self, make: fn() -> C) -> Self {
    self.built_by = Some(make);
    self
  }

  /// Runs every check the cache's capacity can express, panicking on the first violation.
  ///
  /// Runs the whole contract once against a cache built by [`Cache::new`], and — if
  /// [`also_built_by`](Self::also_built_by) supplied a second constructor — once more against a
  /// cache built by that one.
  ///
  /// # Panics
  ///
  /// Panics — naming the check, the capacity, the constructor, and the expected-vs-got values —
  /// the moment a contract law fails. Returns normally on full conformance.
  pub fn run(&self) {
    Self {
      source: self.source,
      name: self.name,
      built_by: None,
      via: "",
      lex_multiple: self.lex_multiple,
      _cache: PhantomData,
      _lang: PhantomData,
    }
    .run_pass();

    if self.built_by.is_some() {
      Self {
        source: self.source,
        name: self.name,
        built_by: self.built_by,
        via: " built by with_options",
        lex_multiple: self.lex_multiple,
        _cache: PhantomData,
        _lang: PhantomData,
      }
      .run_pass();
    }
  }

  /// The cache under test, built the way this pass builds it.
  fn make(&self) -> C {
    match self.built_by {
      Some(f) => f(),
      None => C::new(),
    }
  }

  /// The cache's name plus this pass's constructor, for every failure message.
  fn label(&self) -> Label {
    Label {
      name: self.name,
      via: self.via,
    }
  }

  /// One whole pass of the contract, against caches built by [`make`](Self::make).
  fn run_pass(&self) {
    let name = self.label();
    let cap = self.make().remaining();
    // Past the residency the kit needs a full peek window of tokens the cache is NOT holding:
    // check 6 prefills the peek buffer with them, at every depth from one entry to a buffer with
    // no room left. A capacity-0 cache runs no peek check and needs only the two an ordering law
    // takes to be observable at all.
    let window = <<PeekWindow as Window>::CAPACITY as Unsigned>::USIZE;
    let want = if cap == 0 {
      2
    } else {
      cap.saturating_add(window)
    };
    let corpus = self.corpus(want);
    assert!(
      corpus.len() >= want,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but this cache's capacity is {cap} and the kit needs {want} — the residency plus a {window}-token peek window behind it, so that the refusal laws are observable and the peek buffer can be prefilled, at every depth, with tokens the cache is not itself holding. This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );

    self.check_empty(cap, "fresh");
    self.check_retains_front(cap);
    self.check_empty_push_front(cap);
    self.check_fifo_and_length(cap);
    self.check_pop_order(cap);
    self.check_push_front(cap);
    self.check_peek(cap);
    self.check_peek_across_mutations(cap);
    self.check_clear(cap);
    self.check_span(cap);
    self.check_wrapped_ring(cap);
    self.check_peek_behind_earlier_entries(cap);
    self.check_pop_front_if(cap);
    self.check_push_many(cap);
  }

  // ── the corpus ──────────────────────────────────────────────────────────────────

  /// Lexes up to `want` successful items out of the source, each paired with the state that
  /// produced it — exactly the shape the input layer caches.
  ///
  /// # The loop counts **attempts**, and the `want` gate counts pushes
  ///
  /// Those are different quantities and the difference is a hang. An `Err` does not grow `out`, so
  /// `out.len() < want` never becomes false for a lexer that returns errors forever — and such a
  /// lexer never returns `None` either, so the `break` is unreachable too. The gate is on the
  /// filtered subset while the loop consumes the whole item stream, which is the same shape as an
  /// item budget read at the `next()` boundary while `next()` loops over the errors it accepts.
  ///
  /// So the attempts carry their own ceiling. Unlike the drive-side tally in the parent module,
  /// this counter and the `lex()` it counts are in the **same loop body**, so it bounds its loop by
  /// construction and needs nothing shared: the loops above it are the kit's fixed list of checks.
  ///
  /// # The ceiling is configured, because there is no invariant here to derive it from
  ///
  /// `lex_multiple * units + BUDGET_FLOOR`, and the multiple is the caller's
  /// ([`lex_attempts_multiple`](Self::lex_attempts_multiple)) precisely because the number cannot
  /// be derived. It used to be justified as "monotone progress and nonempty spans bound a
  /// conforming lexer at one item per source unit"; the contract enforced elsewhere says
  /// *non-decreasing* starts and *individually* nonempty spans, which permits overlapping and
  /// repeated-start items, so a finite terminating lexer may legitimately emit many items per unit.
  /// Nothing in this kit checks even that much — it drives the raw lexer and never runs the
  /// lexer-contract tier. The default is a nontermination guard with a generous constant, not a
  /// consequence of anything, and it is overridable for the lexer that legitimately exceeds it.
  ///
  /// # The ceiling has to be a number `attempts` can pass
  ///
  /// It is computed with checked arithmetic, and the multiple it scales is refused above
  /// [`MAX_BUDGET_MULTIPLE`](super::MAX_BUDGET_MULTIPLE), because the *configured* ceiling used to
  /// be able to switch this loop off outright. `lex_attempts_multiple(usize::MAX)` saturated to a
  /// limit of `usize::MAX`, and under the **`attempts > limit`** order of the day no `usize` could
  /// satisfy that comparison: the endless-error lexer this guard exists to refuse ran forever,
  /// with the counter wrapping through zero under a release profile, so the arithmetic did not
  /// stop it either. A guard whose own knob can switch it off is not a guard, and the failure is
  /// silent: the run does not report anything, it simply never ends.
  ///
  /// The comparison is now `attempts >= limit`, asked **before** the increment, and a `usize` does
  /// reach `usize::MAX` — so such a ceiling is enforced, after `usize::MAX` attempts, without the
  /// counter wrapping. That is still not a refusal anybody is present for, which is why the cap
  /// stays; but the cap is what keeps the guard *useful*, not what keeps it alive. Nothing in this
  /// loop wraps at any setting the builder accepts, and nothing wraps at the ones it does not.
  ///
  /// # Panics
  ///
  /// Panics — tagged `lex-budget` — when the ceiling is spent. The alternative is not a shorter
  /// corpus; it is a kit that never returns.
  ///
  /// Panics, separately and tagged `kit-capacity`, when `lex_multiple * units + BUDGET_FLOOR` does
  /// not fit in a `usize`. With the multiple capped that needs a source of more than
  /// `usize::MAX / 65536` units, so what it refuses is not a corpus anyone can build. The tag
  /// differs because the outcome does: no lexing has happened, so nothing has been learned about
  /// the lexer, and a `lex-budget` tag on a failure of the kit's own arithmetic reads as a verdict
  /// on the caller's code. The message names the knob to lower rather than reporting a number
  /// nobody configured.
  fn corpus(&self, want: usize) -> Vec<CachedTokenOf<'inp, L>> {
    let name = self.label();
    let units = self.source.slice(..).map(|s| s.len()).unwrap_or(0);
    let limit = super::representable_budget(self.lex_multiple, units).unwrap_or_else(|| {
      panic!(
        "tokora cache conformance [{name} kit-capacity]: INCONCLUSIVE — the corpus builder's \
         ceiling for a source of {units} units at a multiple of {} does not fit in a usize. This \
         is a limit of the kit's arithmetic and NOT a verdict on the lexer, which has not been \
         asked to lex once. Lower the multiple with CacheHarness::lex_attempts_multiple: an \
         unrepresentable ceiling has to be replaced by some other number, and usize::MAX is one no \
         run reaches while anybody is waiting for it.",
        self.lex_multiple
      )
    });
    let mut lexer = L::new(self.source);
    let mut out = Vec::new();
    let mut attempts = 0usize;
    while out.len() < want {
      // Checked before the increment, the same order both counters in the parent module use.
      // This one is provably total either way — `representable_budget` returns only values
      // strictly below `usize::MAX`, so `attempts` tops out at `limit` — but "the ceiling is
      // compared before the counter moves" is the rule, and a counter that keeps the rule only
      // because of a bound proved somewhere else is the one that breaks when that bound moves.
      if attempts >= limit {
        let cap = super::MAX_BUDGET_MULTIPLE;
        panic!(
          "tokora cache conformance [{name} lex-budget]: building the corpus asked the lexer to \
           lex more than {limit} times over a source of {units} units without collecting {want} \
           token(s). Every attempt is counted, the errors included — an error does not grow the \
           corpus, so a lexer that keeps returning one never reaches the target and never \
           exhausts. If this lexer does terminate and simply emits many items per source unit, \
           which the contract permits, raise the ceiling with \
           CacheHarness::lex_attempts_multiple; that multiple is capped at {cap} per source \
           unit, above which the kit cannot tell a dense lexer from a nonterminating one."
        );
      }
      attempts += 1;
      let Some(res) = lexer.lex() else { break };
      let span = lexer.span();
      let state = lexer.state().clone();
      if let Ok(tok) = res {
        out.push(CachedToken::new(Spanned::new(span, tok), state));
      }
    }
    out
  }

  /// A fresh cache holding the first `n` corpus tokens, pushed back to front-order.
  ///
  /// Returns the cache and the **entries** it should be holding, so every later assertion
  /// compares against a list the kit built rather than against the cache's own answer. Each entry
  /// is cloned before the push and the clone is what is kept: the cache takes the original by
  /// value, and what the kit needs afterwards is the whole triple — span, token and state — not
  /// the span projection it kept before #183.
  fn filled(&self, n: usize) -> (C, Vec<CachedTokenOf<'inp, L>>) {
    let mut cache = self.make();
    let mut want = Vec::new();
    for (i, tok) in self.corpus(n).into_iter().enumerate() {
      // A refusal here is expected — `filled` is driven past the capacity — so this is the arm
      // that keeps what was accepted rather than the one that asserts acceptance. Either way the
      // `Ok` reference is read: see [`checked_push`](Self::checked_push).
      if let Ok(placed) = self.checked_push_back(
        &mut cache,
        tok,
        &format!("push_back #{i} filling a fresh cache"),
      ) {
        want.push(placed);
      }
    }
    (cache, want)
  }

  // ── every value a `Cache` method hands back is read, not counted ─────────────────
  //
  // A value reduced to `is_ok()`/`is_some()` is a value the kit did not check, and #183's own
  // review found that class twice in two rounds — first a pop whose entry was discarded
  // (`check_peek_across_mutations`), then a push whose returned reference was (every push in the
  // kit). Both were invisible for the same reason: everything downstream observes **storage**,
  // and neither the popped entry nor the returned reference is storage.
  //
  // So the class is closed rather than the occurrences — and closed by ENUMERATION, not by
  // looking harder, which is the difference between a class that is finished and one that keeps
  // producing findings. A `Cache` method returns an `Option` or a `Result`, so there are exactly
  // four shapes a value can arrive in, and every one of them is now accounted for:
  //
  //   * `Option::Some`  — compared, through the entry comparison below.
  //   * `Option::None`  — presence-only, at twelve sites, because **absence is the law under
  //                       test**: a drained cache that must not answer, an empty cache that must
  //                       not answer. Nothing came back, so presence is not a weaker check there
  //                       — it is the whole check. Each carries a comment at the site.
  //
  //                       That bound is on the RETURN, and #183's ninth round found it being
  //                       read as a bound on the CALL. `pop_front_if` and `try_pop_front_if` —
  //                       the only two `Cache` methods that run CALLER CODE, which is a closed
  //                       list read off the trait rather than an inspection of this file — hand
  //                       an entry over *before* they learn what to return. A `None` therefore
  //                       says nothing about what the predicate was shown, and check 9's
  //                       DECLINING arm is compared rather than counted for exactly that
  //                       reason: it used to sit in this bullet, and a cache could answer
  //                       `None`, remove nothing, keep every span in place, and still run the
  //                       caller's validation predicate against an entry it never held there.
  //
  //                       The two empty-cache probes stay in this bullet on a different footing
  //                       and are sound: their law is that the predicate must NOT RUN at all,
  //                       asserted directly beside them, so on any conforming path nothing is
  //                       handed to anyone. A cache that runs one anyway fails on the run, the
  //                       louder violation; what it was shown is then not compared, which is
  //                       claim accuracy and not a certification any cache can pass.
  //   * `Result::Ok`    — compared, by [`checked_push`](Self::checked_push).
  //   * `Result::Err`   — compared. At the sites that drive a refusal ON PURPOSE (checks 3 and 5)
  //                       the caller compares it in its own round-trip wording; at the sites that
  //                       expected acceptance,
  //                       [`unexpected_refusal`](Self::unexpected_refusal) compares it before the
  //                       refusal panic can discard it.
  //
  // There is no fifth shape. But an enumeration by shape is not an enumeration by SITE, and that
  // is where four review rounds in a row went wrong: a single call site can be two shapes at
  // once — checks 3 and 5 each have one `push_back`/`push_front` whose `Err` is *sometimes*
  // legitimate and sometimes not — so an enumeration organised by shape classified them once, as
  // "expected refusal", and never saw the other branch.
  //
  // So the account above is no longer the guarantee. `CACHE_CALL_CENSUS` in `cache_tests.rs` is:
  // it parses this file and fails, naming the function and the line, the moment a call to any of
  // these methods appears at a site its table does not know about. Adding a call somewhere new
  // is a red test, not a missed review comment. The helpers below exist so that the registered
  // sites are few and each one's disposition is obvious at the call.

  /// The `Ok` arm of an accepted push, compared against the entry that was offered.
  ///
  /// [`Cache::push_back`] and [`Cache::push_front`] both promise `Ok` with **a reference to the
  /// cached token**, so that reference is a contract value like any other — and until #183's
  /// second review round every push in this kit reduced it to `is_ok()`. A cache can store the
  /// offered entry perfectly and hand back a reference assembled from a *different* resident, and
  /// nothing downstream can tell: `front`, `back`, the pops and `peek` all read storage, which is
  /// correct, while the value the caller was actually handed is not.
  ///
  /// Both push arms and all sixteen push sites route through this **one** comparison, which is
  /// what lets a single mutant pin the read everywhere it happens. On success the caller gets its
  /// own offered entry back as the expectation to keep — the same value it handed over, now
  /// checked against what the cache says it placed. A refusal is passed straight through: the
  /// round-trip is a different law, asserted by the callers that drive a refusal on purpose.
  fn checked_push(
    &self,
    result: Result<CachedTokenRefOf<'_, 'inp, L>, CachedTokenOf<'inp, L>>,
    expectation: CachedTokenOf<'inp, L>,
    site: &str,
  ) -> Result<CachedTokenOf<'inp, L>, CachedTokenOf<'inp, L>> {
    let name = self.label();
    match result {
      Ok(placed) => {
        let placed_span = (**placed.token().span_ref()).clone();
        let offered_span = span_of::<L>(&expectation);
        self.assert_ref_entry_eq(
          &placed,
          &expectation,
          &format!("accepted-push {site}"),
          format_args!(
            "tokora cache conformance [{name} accepted-push] {site}: the push was accepted and returned a reference to {placed_span:?}, expected the entry it was just handed, {offered_span:?}. `push_back`/`push_front` promise `Ok` with a reference to the token they cached."
          ),
          NO_NOTE,
        );
        Ok(expectation)
      }
      Err(returned) => Err(returned),
    }
  }

  /// The one place in the kit that calls [`Cache::push_back`], and the one place that decides
  /// what an `Err` from it means.
  ///
  /// `must_accept` is the caller's expectation at THIS push, which is what the two mixed sites
  /// (checks 3 and 5) need: the same call there can produce a warranted refusal or an
  /// unwarranted one, and until #183's fifth review round the unwarranted branch asserted its
  /// way out before the returned entry reached any comparison. The split is:
  ///
  /// * accepted — the `Ok` reference is compared against the offered entry
  ///   ([`checked_push`](Self::checked_push)), and the offered entry is handed back as the
  ///   caller's expectation;
  /// * refused where a refusal is legitimate — passed straight through, because the round-trip
  ///   is a different law and the caller asserts it in its own wording (that is where
  ///   `REFUSAL_STATE_SWAP` and `REFUSAL_TOKEN_SWAP` fire, and moving the comparison here would
  ///   steal their site);
  /// * refused where the contract required acceptance —
  ///   [`unexpected_refusal`](Self::unexpected_refusal), which compares the entry and then
  ///   panics with the site's own refusal wording.
  fn push_back_expecting(
    &self,
    cache: &mut C,
    tok: CachedTokenOf<'inp, L>,
    site: &str,
    must_accept: bool,
    refusal: core::fmt::Arguments<'_>,
  ) -> Result<CachedTokenOf<'inp, L>, CachedTokenOf<'inp, L>> {
    let offered = tok.clone();
    // CACHE_CALL_CENSUS: routed
    match self.checked_push(cache.push_back(tok), offered.clone(), site) {
      Ok(placed) => Ok(placed),
      Err(returned) => {
        if must_accept {
          self.unexpected_refusal(returned, offered, site, refusal)
        } else {
          Err(returned)
        }
      }
    }
  }

  /// [`push_back_expecting`](Self::push_back_expecting) on the prepend arm, and the one place in
  /// the kit that calls [`Cache::push_front`].
  fn push_front_expecting(
    &self,
    cache: &mut C,
    tok: CachedTokenOf<'inp, L>,
    site: &str,
    must_accept: bool,
    refusal: core::fmt::Arguments<'_>,
  ) -> Result<CachedTokenOf<'inp, L>, CachedTokenOf<'inp, L>> {
    let offered = tok.clone();
    // CACHE_CALL_CENSUS: routed
    match self.checked_push(cache.push_front(tok), offered.clone(), site) {
      Ok(placed) => Ok(placed),
      Err(returned) => {
        if must_accept {
          self.unexpected_refusal(returned, offered, site, refusal)
        } else {
          Err(returned)
        }
      }
    }
  }

  /// A `push_back` at a site where a refusal is a case to handle rather than a violation.
  fn checked_push_back(
    &self,
    cache: &mut C,
    tok: CachedTokenOf<'inp, L>,
    site: &str,
  ) -> Result<CachedTokenOf<'inp, L>, CachedTokenOf<'inp, L>> {
    self.push_back_expecting(
      cache,
      tok,
      site,
      false,
      format_args!("refusal is legitimate here"),
    )
  }

  /// A `push_back` the contract says cannot be refused, with the site's own refusal wording.
  ///
  /// Returns the entry the caller offered, once the accepted push's `Ok` reference has been
  /// checked against it — so the expectation lists the callers build are the same values they
  /// handed over, and the cache has agreed it placed them.
  fn accepted_push_back(
    &self,
    cache: &mut C,
    tok: CachedTokenOf<'inp, L>,
    site: &str,
    refusal: core::fmt::Arguments<'_>,
  ) -> CachedTokenOf<'inp, L> {
    match self.push_back_expecting(cache, tok, site, true, refusal) {
      Ok(placed) => placed,
      Err(_) => unreachable!(
        "push_back_expecting diverges through unexpected_refusal when must_accept is set"
      ),
    }
  }

  /// [`accepted_push_back`](Self::accepted_push_back) on the prepend arm.
  fn accepted_push_front(
    &self,
    cache: &mut C,
    tok: CachedTokenOf<'inp, L>,
    site: &str,
    refusal: core::fmt::Arguments<'_>,
  ) -> CachedTokenOf<'inp, L> {
    match self.push_front_expecting(cache, tok, site, true, refusal) {
      Ok(placed) => placed,
      Err(_) => unreachable!(
        "push_front_expecting diverges through unexpected_refusal when must_accept is set"
      ),
    }
  }

  /// The refusal a site did not expect: the entry that came back is **compared first**, and the
  /// site's own refusal message is the panic that follows.
  ///
  /// The refusal is the louder violation and the message the callers pin names it, so it is what
  /// this ends on. But the value handed back is still a value the kit received, and dropping it
  /// on the way to the panic was the last place in the kit that took a `Cache` return and looked
  /// only at its shape.
  ///
  /// **Deliberately not in [`checked_push`](Self::checked_push)'s `Err` arm.** The sites that
  /// drive a refusal *on purpose* — check 3's and check 5's round-trips — already compare there,
  /// in their own wording, and that is where `REFUSAL_STATE_SWAP` and `REFUSAL_TOKEN_SWAP` fire.
  /// Moving the comparison up into the shared helper would run it twice and would relocate those
  /// two cells' firing site, unpinning the round-trip messages their `expected` strings name —
  /// the same pin-theft that `STATE_LAG` nearly suffered when the accepted-push comparison was
  /// added. So it lives here, on the path no caller compares, and nowhere else.
  ///
  /// No cache reaches this by conforming: getting here needs a refusal the contract does not
  /// allow *and* a corrupted return, and the refusal alone already fails the kit. It closes a
  /// claim rather than a hole — see the shape enumeration in the module docs.
  fn unexpected_refusal(
    &self,
    returned: CachedTokenOf<'inp, L>,
    offered: CachedTokenOf<'inp, L>,
    site: &str,
    refusal: core::fmt::Arguments<'_>,
  ) -> ! {
    let name = self.label();
    let returned_span = span_of::<L>(&returned);
    let offered_span = span_of::<L>(&offered);
    self.assert_owned_entry_eq(
      &returned,
      &offered,
      &format!("unexpected-refusal {site}"),
      format_args!(
        "tokora cache conformance [{name} unexpected-refusal] {site}: the push was refused when the contract required it be accepted, and it handed back {returned_span:?} rather than the entry it was offered, {offered_span:?}. A refusal must hand the caller its token unchanged, warranted or not."
      ),
      NO_NOTE,
    );
    panic!("{refusal}")
  }

  /// A pop whose answer the kit already knows: it must answer, and it must answer with `want`.
  /// The one place in the kit that calls [`Cache::pop_front`] for a value it intends to keep.
  ///
  /// The residency sweeps drive runs of pops purely to *reach* a residency, and until #183's
  /// second review round each of those asserted `is_some()` and threw the entry away. The entry
  /// is never unknown at those sites — it is the next one off the end being drained — so there is
  /// nothing to justify not comparing it, and a pop that removes the right entry while returning
  /// a wrong one is precisely the class this kit exists to catch.
  fn drained_front(
    &self,
    cache: &mut C,
    want: &CachedTokenOf<'inp, L>,
    site: &str,
    absent: core::fmt::Arguments<'_>,
    tail: &str,
  ) -> CachedTokenOf<'inp, L> {
    // CACHE_CALL_CENSUS: routed
    let popped = cache.pop_front();
    self.drained(popped, want, site, absent, tail, NO_NOTE)
  }

  /// [`drained_front`](Self::drained_front) at the other end, and the one place the kit calls
  /// [`Cache::pop_back`] for a value it intends to keep.
  ///
  /// The restore note is not a parameter here: `pop_back` **is** the input layer's rollback
  /// path, at every site, so the note belongs to the end rather than to the caller — one fewer
  /// thing a new drain can forget.
  fn drained_back(
    &self,
    cache: &mut C,
    want: &CachedTokenOf<'inp, L>,
    site: &str,
    absent: core::fmt::Arguments<'_>,
    tail: &str,
  ) -> CachedTokenOf<'inp, L> {
    // CACHE_CALL_CENSUS: routed
    let popped = cache.pop_back();
    self.drained(popped, want, site, absent, tail, RESTORE_NOTE)
  }

  /// The shared body of the two drains above: the pop must answer, and it must answer with
  /// `want` in all three components.
  ///
  /// `tail` is the site's own explanatory sentence, appended to a uniform lede so each drain
  /// keeps the rationale it had before the routing — what a wrong answer means at *that* site —
  /// while the lede and the entry comparison stay in one place.
  fn drained(
    &self,
    popped: Option<CachedTokenOf<'inp, L>>,
    want: &CachedTokenOf<'inp, L>,
    site: &str,
    absent: core::fmt::Arguments<'_>,
    tail: &str,
    state_note: &str,
  ) -> CachedTokenOf<'inp, L> {
    let name = self.label();
    let Some(popped) = popped else {
      panic!("{absent}")
    };
    let got_span = span_of::<L>(&popped);
    let want_span = span_of::<L>(want);
    self.assert_owned_entry_eq(
      &popped,
      want,
      site,
      format_args!(
        "tokora cache conformance [{name} drain] {site}: the pop removed {got_span:?}, expected {want_span:?}.{tail}"
      ),
      state_note,
    );
    popped
  }

  /// The `front()` edge read, compared in full — and the one place the kit calls
  /// [`Cache::front`].
  fn assert_front_entry(&self, cache: &C, want: Option<&CachedTokenOf<'inp, L>>, when: &str) {
    let name = self.label();
    // CACHE_CALL_CENSUS: routed
    match (want, cache.front()) {
      (Some(expected), Some(got)) => {
        let expected_span = span_of::<L>(expected);
        let got_span = (**got.token().span_ref()).clone();
        self.assert_ref_entry_eq(
          &got,
          expected,
          &format!("edge-identity front() {when}"),
          format_args!(
            "tokora cache conformance [{name} edge-identity] {when}: front() names {got_span:?}, expected the OLDEST resident entry {expected_span:?} — the same entry front_span() must name"
          ),
          NO_NOTE,
        );
      }
      (None, None) => {}
      (a, b) => panic!(
        "tokora cache conformance [{name} edge-identity] {when}: front() presence disagrees — expected {:?}, got {:?}",
        a.map(span_of::<L>),
        b.map(|t| (**t.token().span_ref()).clone())
      ),
    }
  }

  /// The `back()` edge read — the entry `InputRef::resume` rebuilds a lexer from, which is why
  /// its state message carries [`RESTORE_NOTE`]. The one place the kit calls [`Cache::back`].
  fn assert_back_entry(&self, cache: &C, want: Option<&CachedTokenOf<'inp, L>>, when: &str) {
    let name = self.label();
    // CACHE_CALL_CENSUS: routed
    match (want, cache.back()) {
      (Some(expected), Some(got)) => {
        let expected_span = span_of::<L>(expected);
        let got_span = (**got.token().span_ref()).clone();
        self.assert_ref_entry_eq(
          &got,
          expected,
          &format!("edge-identity back() {when}"),
          format_args!(
            "tokora cache conformance [{name} edge-identity] {when}: back() names {got_span:?}, expected the NEWEST resident entry {expected_span:?} — the same entry back_span() must name"
          ),
          RESTORE_NOTE,
        );
      }
      (None, None) => {}
      (a, b) => panic!(
        "tokora cache conformance [{name} edge-identity] {when}: back() presence disagrees — expected {:?}, got {:?}",
        a.map(span_of::<L>),
        b.map(|t| (**t.token().span_ref()).clone())
      ),
    }
  }

  /// `peek_one` as the triple the kit compares — the one place it is called for a value.
  fn peeked_one(&self, cache: &C) -> Option<PeekedTriple<'inp, L>> {
    // CACHE_CALL_CENSUS: routed
    cache.peek_one().map(|one| Self::triple(&one))
  }

  /// `pop_front_if` driven with a **recording** predicate, its argument compared against the
  /// front entry before anything else is asserted. The one place the kit calls it for a value.
  ///
  /// Routed rather than written inline at each site so the predicate argument cannot be
  /// discarded by a future edit: the closure lives here, and the comparison is unconditional.
  fn recording_pop_front_if(
    &self,
    cache: &mut C,
    want_front: &CachedTokenOf<'inp, L>,
    answer: bool,
    site: &str,
    lede: &str,
  ) -> Option<CachedTokenOf<'inp, L>> {
    let seen: Cell<Option<PeekedTriple<'inp, L>>> = Cell::new(None);
    // CACHE_CALL_CENSUS: routed
    let popped = cache.pop_front_if(|tok| {
      seen.set(Some(Self::ref_triple(&tok)));
      answer
    });
    self.assert_recorded_predicate_arg(seen.take().as_ref(), want_front, site, lede);
    popped
  }

  /// [`recording_pop_front_if`](Self::recording_pop_front_if)'s twin on the fallible arm.
  fn recording_try_pop_front_if(
    &self,
    cache: &mut C,
    want_front: &CachedTokenOf<'inp, L>,
    outcome: Result<(), &'static str>,
    site: &str,
    lede: &str,
  ) -> Option<Result<CachedTokenOf<'inp, L>, &'static str>> {
    let seen: Cell<Option<PeekedTriple<'inp, L>>> = Cell::new(None);
    // CACHE_CALL_CENSUS: routed
    let popped = cache.try_pop_front_if::<&'static str, _>(|tok| {
      seen.set(Some(Self::ref_triple(&tok)));
      outcome
    });
    self.assert_recorded_predicate_arg(seen.take().as_ref(), want_front, site, lede);
    popped
  }

  // ── the entry comparison every oracle reads its answers through ─────────────────

  /// The kit's fundamental comparison: a cached entry is the triple (span, token, state), and a
  /// cache that hands one back has to hand back all three of them (#183).
  ///
  /// `span_msg` is the calling site's own span-half message, kept **verbatim** from before the
  /// entry observable existed, so the cells that pin its wording (`front() names`, `OLDEST
  /// FIRST`, `a refused push_back returned a DIFFERENT token`, …) keep matching. The token and
  /// state halves get tags of their own — `entry-token` and `entry-state` — so each of the three
  /// components fails with its own greppable message and a mutant can pin exactly the comparison
  /// it exists to exercise.
  ///
  /// **Span first, always.** A defect that moves an entry to the wrong position corrupts all
  /// three components at once, and it is an *ordering* violation; reporting it as a token
  /// mismatch would bury the diagnosis and break every existing expectation. The token and state
  /// halves therefore only ever speak about an entry the span half has already agreed is the
  /// right one.
  ///
  /// `state_note` is [`RESTORE_NOTE`] at the `back()`/`pop_back` sites and [`NO_NOTE`] everywhere
  /// else.
  fn assert_entry_eq(
    &self,
    got: (&L::Span, &L::Token, &L::State),
    want: &CachedTokenOf<'inp, L>,
    site: &str,
    span_msg: core::fmt::Arguments<'_>,
    state_note: &str,
  ) {
    let name = self.label();
    let (got_span, got_token, got_state) = got;
    // Bound rather than chained: `token()` builds a `Spanned<&T, &Span>` by value, so reading
    // through it in the assertion below would borrow from a temporary that is gone by the time
    // the message is formatted.
    let want_entry = want.token();
    let want_span: &L::Span = want_entry.span_ref();
    let want_token: &L::Token = want_entry.data();
    assert!(got_span == want_span, "{span_msg}");
    assert!(
      got_token == want_token,
      "tokora cache conformance [{name} entry-token] {site}: the entry at {want_span:?} is a DIFFERENT token from the one stored there — expected {want_token:?}, got {got_token:?}. A cached entry is the triple (span, token, state); a right span with someone else's token beside it is a permuted cache, not a conforming one."
    );
    assert!(
      got_state == want.state(),
      "tokora cache conformance [{name} entry-state] {site}: the entry at {want_span:?} carries a DIFFERENT L::State from the one stored there — expected {:?}, got {got_state:?}.{state_note}",
      want.state()
    );
  }

  /// [`assert_entry_eq`](Self::assert_entry_eq) against an **owned** entry — what `pop_front`,
  /// `pop_back`, a refused push and `push_many`'s overflow iterator hand back.
  fn assert_owned_entry_eq(
    &self,
    got: &CachedTokenOf<'inp, L>,
    want: &CachedTokenOf<'inp, L>,
    site: &str,
    span_msg: core::fmt::Arguments<'_>,
    state_note: &str,
  ) {
    self.assert_entry_eq(
      (*got.token().span_ref(), *got.token().data(), got.state()),
      want,
      site,
      span_msg,
      state_note,
    );
  }

  /// [`assert_entry_eq`](Self::assert_entry_eq) against a **borrowed** entry — what `front`,
  /// `back` and the `pop_front_if` predicates are handed.
  ///
  /// `CachedTokenRefOf`'s own parameters are already references, so every accessor adds one more
  /// level than the owned form does; that is the whole of the extra `*`s here.
  fn assert_ref_entry_eq(
    &self,
    got: &CachedTokenRefOf<'_, 'inp, L>,
    want: &CachedTokenOf<'inp, L>,
    site: &str,
    span_msg: core::fmt::Arguments<'_>,
    state_note: &str,
  ) {
    self.assert_entry_eq(
      (**got.token().span_ref(), **got.token().data(), *got.state()),
      want,
      site,
      span_msg,
      state_note,
    );
  }

  /// [`assert_entry_eq`](Self::assert_entry_eq) against one **peeked** entry, flattened to the
  /// triple by [`peeked_entries_through`](Self::peeked_entries_through) so that the borrowed and
  /// owned arms of [`Maybe`] are read the same way.
  fn assert_peeked_entry_eq(
    &self,
    got: &PeekedTriple<'inp, L>,
    want: &CachedTokenOf<'inp, L>,
    site: &str,
    span_msg: core::fmt::Arguments<'_>,
  ) {
    self.assert_entry_eq((&got.0, &got.1, &got.2), want, site, span_msg, NO_NOTE);
  }

  /// The token and state halves of a whole peeked run, position by position.
  ///
  /// The span half is compared by the caller, as one vector against another, because that is the
  /// assertion whose wording check 6's cells pin (`OLDEST FIRST`) and whose message shows the
  /// reader the run rather than one entry of it. This walks the same positions afterwards, so a
  /// re-association inside a run whose spans are all in the right place still fails.
  fn assert_peeked_run_eq(
    &self,
    got: &[PeekedTriple<'inp, L>],
    want: &[CachedTokenOf<'inp, L>],
    site: &str,
  ) {
    for (i, (entry, expected)) in got.iter().zip(want).enumerate() {
      self.assert_peeked_entry_eq(
        entry,
        expected,
        site,
        format_args!(
          "tokora cache conformance kit bug [{} bounded-peek] {site}: position {i} of the peeked run has a span the vector comparison above should already have rejected",
          self.label()
        ),
      );
    }
  }

  // ── 1. empty invariants ─────────────────────────────────────────────────────────

  fn check_empty(&self, cap: usize, when: &str) {
    let mut cache = self.make();
    self.assert_empty(&mut cache, cap, when);
  }

  /// Takes `cache` by `&mut` specifically so its own pop methods can be probed below — see
  /// there for why a fresh `C::new()` used to stand in for it and what that missed.
  fn assert_empty(&self, cache: &mut C, cap: usize, when: &str) {
    let name = self.label();
    assert!(
      cache.len() == 0,
      "tokora cache conformance [{name} empty-invariants/{when}]: len() is {}, expected 0",
      cache.len()
    );
    assert!(
      cache.is_empty(),
      "tokora cache conformance [{name} empty-invariants/{when}]: is_empty() is false while len() is 0"
    );
    assert!(
      cache.remaining() == cap,
      "tokora cache conformance [{name} empty-invariants/{when}]: remaining() is {}, expected the empty-cache capacity {cap}. `remaining` must be exactly how many more push_back calls will be accepted.",
      cache.remaining()
    );
    // Presence-only by construction, here and for the three probes below: the law under test is
    // that these do NOT answer, so there is no entry behind any of them for the entry comparison
    // to read. This is the documented exception to "every value a Cache method hands back is
    // read" — absence is the whole observable, not a weaker view of one.
    assert!(
      // CACHE_CALL_CENSUS: absence
      cache.front().is_none(),
      "tokora cache conformance [{name} empty-invariants/{when}]: front()/back() answered on an empty cache — front() did"
    );
    assert!(
      // CACHE_CALL_CENSUS: absence
      cache.back().is_none(),
      "tokora cache conformance [{name} empty-invariants/{when}]: front()/back() answered on an empty cache — back() did"
    );
    assert!(
      cache.front_span().is_none() && cache.back_span().is_none(),
      "tokora cache conformance [{name} empty-invariants/{when}]: front_span()/back_span() answered on an empty cache"
    );
    assert!(
      cache.span().is_none(),
      "tokora cache conformance [{name} empty-invariants/{when}]: span() answered on an empty cache"
    );
    // The cache UNDER TEST, not a fresh `C::new()`: at `check_empty`'s call site the two are the
    // same cache anyway, but at `check_clear`'s they are not, and a fresh cache answering for
    // one it never touched proves nothing about whether THIS cache's pop methods still work —
    // or still refuse to answer — after whatever produced its current empty state (#180 part A,
    // item 2).
    // One assertion per end rather than one `&&` over both: `&&` short-circuits, so a cache that
    // wrongly answers on `pop_front` would leave `pop_back` unprobed and its own defect unnamed.
    // Split, each end is called and each failure says which one answered.
    assert!(
      // CACHE_CALL_CENSUS: absence
      cache.pop_front().is_none(),
      "tokora cache conformance [{name} empty-invariants/{when}]: a pop answered on an empty cache — pop_front() did"
    );
    assert!(
      // CACHE_CALL_CENSUS: absence
      cache.pop_back().is_none(),
      "tokora cache conformance [{name} empty-invariants/{when}]: a pop answered on an empty cache — pop_back() did"
    );
    assert!(
      // CACHE_CALL_CENSUS: absence
      cache.peek_one().is_none(),
      "tokora cache conformance [{name} empty-invariants/{when}]: peek_one() answered on an empty cache"
    );
    assert!(
      self.peeked_entries(cache).is_empty(),
      "tokora cache conformance [{name} empty-invariants/{when}]: peek() appended entries from an empty cache"
    );
  }

  // ── 2. RETAINS_FRONT honesty ────────────────────────────────────────────────────

  fn check_retains_front(&self, cap: usize) {
    if !C::RETAINS_FRONT {
      return;
    }
    let name = self.label();
    assert!(
      cap >= 1,
      "tokora cache conformance [{name} retains-front]: RETAINS_FRONT is declared true but the empty-cache capacity is 0"
    );

    // The law verbatim, on a cache that has never seen an operation: the `push_front` under test
    // is the first thing that happens to it. Reaching an empty cache through a push_back and a
    // pop_front first — which is what the second half below does — would let a cache that
    // refuses the *first-ever* front push and accepts a later one pass: a slot initialised
    // lazily on the first `push_back`, a "warm-up" flag, a front link only a back push
    // establishes. The input layer's first put-back can land on exactly that cache, and it has
    // already compiled the parked-slot fallback out.
    let mut fresh = self.make();
    let first = self
      .corpus(1)
      .pop()
      .expect("run() checked the corpus is non-empty");
    // The whole entry, cloned before the push hands the original to the cache: what
    // `assert_resident` compares is the triple, so the expectation has to carry all of it.
    let want_first = self.accepted_push_front(
      &mut fresh,
      first,
      "push_front into a fresh cache, retains-front",
      format_args!(
        "tokora cache conformance [{name} retains-front]: the FIRST operation on a fresh cache — a push_front while it is empty — was refused while RETAINS_FRONT is declared true. The input layer compiles its parked-slot fallback OUT on that declaration, so a refusal here loses the token — declare `false` or retain the front."
      ),
    );
    self.assert_resident(
      &fresh,
      cap,
      core::slice::from_ref(&want_first),
      "after the first-ever operation, a push_front into a fresh cache",
    );

    // And again on an empty cache that has been used: a cache that retains the front until it is
    // drained once, and then stops, is the same violation reached from the other side.
    let mut cache = self.make();
    let tok = self.corpus(1).pop().expect("the corpus is non-empty");
    let seeded = self.accepted_push_back(
      &mut cache,
      tok,
      "push_back seeding an empty cache, retains-front",
      format_args!(
        "tokora cache conformance [{name} retains-front]: push_back into an empty cache was refused while RETAINS_FRONT is true"
      ),
    );
    // The entry the pop hands back is the one that was just pushed, and it is compared as such:
    // it is re-pushed on the next line and becomes the residency the caller then reasons about,
    // so a corrupt pop here would seed the expectation with its own corruption.
    let popped = self.drained_front(
      &mut cache,
      &seeded,
      "retains-front pop_front off a one-entry cache",
      format_args!(
        "tokora cache conformance [{name} retains-front]: the pop that empties a one-entry cache answered None"
      ),
      " The entry is re-pushed on the next line and becomes the residency this check then reasons about, so a corrupt pop here would seed the expectation with its own corruption.",
    );
    self.accepted_push_front(
      &mut cache,
      popped,
      "push_front into an emptied cache, retains-front",
      format_args!(
        "tokora cache conformance [{name} retains-front]: push_front into an emptied cache was refused while RETAINS_FRONT is declared true. The input layer compiles its parked-slot fallback OUT on that declaration, so a refusal here loses the token — declare `false` or retain the front."
      ),
    );
  }

  // ── check 5's refusal law where check 5 cannot reach it: an empty cache ─────────

  /// Check 5's refusal law against an **empty** cache, at every nonzero capacity: a push is
  /// refused only when the cache is FULL, and an empty cache with capacity is not full.
  ///
  /// The state has to be driven here because neither check that touches it covers it.
  /// [`check_push_front`](Self::check_push_front) seeds the cache with a `push_back` before its
  /// first front push — a prepend law needs something to prepend before — so the empty cache is
  /// never the one a front push meets there. [`check_retains_front`](Self::check_retains_front)
  /// does meet it, but only for a cache that **declares** `RETAINS_FRONT`, and returns at its
  /// first line for one that does not. Between the two, a nonzero-capacity cache could declare
  /// `false`, refuse the first front push into an empty cache, and accept every later one, and
  /// nothing in the kit would say so.
  ///
  /// So the *declaration* check and the *queue law* are separate. This is the law, and it holds
  /// whatever `RETAINS_FRONT` says; the declaration changes only what a refusal **costs** the
  /// input layer — a parked slot where it is `false`, a lost token where it is `true` — which is
  /// why check 2 keeps its own wording for the caches that declare it.
  fn check_empty_push_front(&self, cap: usize) {
    if cap == 0 {
      // Always full, so every refusal is warranted. The one capacity at which a cache may
      // legitimately refuse a front push into an empty cache is the one where "empty" and "full"
      // are the same state.
      return;
    }
    let name = self.label();
    let declares = C::RETAINS_FRONT;

    // Fresh: the front push is the first operation this cache has ever seen.
    let mut fresh = self.make();
    let tok = self
      .corpus(1)
      .pop()
      .expect("run() checked the corpus is non-empty");
    let want_fresh = self.accepted_push_front(
      &mut fresh,
      tok,
      "push_front into a fresh, empty cache",
      format_args!(
        "tokora cache conformance [{name} push-front/into-empty]: a push_front into an EMPTY cache — the first operation this one has seen, with all {cap} slots free — was REFUSED. A push is refused only when the cache is FULL, on this arm exactly as on push_back's, and an empty cache with capacity is not full. RETAINS_FRONT is declared {declares}, which decides what the refusal costs the input layer (a parked slot, or a lost token where the fallback is compiled out), not whether it is allowed."
      ),
    );
    self.assert_resident(
      &fresh,
      cap,
      core::slice::from_ref(&want_fresh),
      "after a push_front into a fresh, empty cache",
    );

    // Used and emptied: the same state reached the other way, for a cache whose front is
    // established lazily and then torn down again with the entry that established it.
    let mut used = self.make();
    let tok = self.corpus(1).pop().expect("the corpus is non-empty");
    let seeded = self.accepted_push_back(
      &mut used,
      tok,
      "push_back seeding an empty cache, push-front/into-empty",
      format_args!(
        "tokora cache conformance [{name} push-front/into-empty]: push_back into an empty cache with {cap} slots free was refused"
      ),
    );
    // Compared against what was seeded, for the reason `check_retains_front`'s twin is: the
    // popped entry is re-pushed and becomes the expectation this check then asserts against, so
    // a corrupt pop would otherwise define its own correctness.
    let popped = self.drained_front(
      &mut used,
      &seeded,
      "push-front/into-empty pop_front off a one-entry cache",
      format_args!(
        "tokora cache conformance [{name} push-front/into-empty]: the pop that empties a one-entry cache answered None"
      ),
      " The entry is re-pushed on the next line and becomes the residency this check then reasons about.",
    );
    let want_used = self.accepted_push_front(
      &mut used,
      popped,
      "push_front into a used, emptied cache",
      format_args!(
        "tokora cache conformance [{name} push-front/into-empty]: a push_front into a cache that was filled and then emptied — all {cap} slots free again — was REFUSED. A push is refused only when the cache is FULL, and this one holds nothing."
      ),
    );
    self.assert_resident(
      &used,
      cap,
      core::slice::from_ref(&want_used),
      "after a push_front into a used, emptied cache",
    );
  }

  // ── 3. FIFO append, exact length, and the refusal round-trip ────────────────────

  fn check_fifo_and_length(&self, cap: usize) {
    let name = self.label();
    let mut cache = self.make();
    let corpus = self.corpus(cap.saturating_add(1).max(2));
    let mut resident: Vec<CachedTokenOf<'inp, L>> = Vec::new();

    for (i, tok) in corpus.into_iter().enumerate() {
      let offered = tok.clone();
      let span = span_of::<L>(&tok);
      let expect_accept = resident.len() < cap;
      // The mixed site: this same call can produce a warranted refusal or an unwarranted one, and
      // `must_accept` is what tells the router which. Before #183's fifth review round the
      // unwarranted branch asserted its way out below and the returned entry was never compared.
      let refusal = format!(
        "tokora cache conformance [{name} exact-length]: push_back #{i} was REFUSED with {} of {cap} slots used; `remaining` promised it would be accepted",
        resident.len()
      );
      match self.push_back_expecting(
        &mut cache,
        tok,
        &format!("push_back #{i}"),
        expect_accept,
        format_args!("{refusal}"),
      ) {
        Ok(placed) => {
          assert!(
            expect_accept,
            "tokora cache conformance [{name} exact-length]: push_back #{i} was ACCEPTED with {} entries resident and a capacity of {cap}; `remaining` had already reached 0, so this push contradicts it",
            resident.len()
          );
          resident.push(placed);
        }
        Err(returned) => {
          // The refusal round-trip: the token comes back unchanged, and nothing moved.
          // "Unchanged" is the whole ENTRY — a refusal that hands back the offered span with
          // somebody else's token or state beside it has changed it (#183).
          let back_span = span_of::<L>(&returned);
          self.assert_owned_entry_eq(
            &returned,
            &offered,
            "refusal-round-trip push_back",
            format_args!(
              "tokora cache conformance [{name} refusal-round-trip]: a refused push_back returned a DIFFERENT token: pushed span {span:?}, got back {back_span:?}. A refusal must hand the caller its token unchanged."
            ),
            NO_NOTE,
          );
          self.assert_resident(&cache, cap, &resident, "after a refused push_back");
        }
      }
      self.assert_resident(&cache, cap, &resident, "after push_back");
    }
    assert!(
      resident.len() == cap,
      "tokora cache conformance [{name} exact-length]: pushing past the capacity left {} entries resident, expected {cap}",
      resident.len()
    );
  }

  /// The four length/edge observables against the list the kit is tracking.
  ///
  /// `want` is the list of **entries** the cache should be holding. The counts and the
  /// `front_span`/`back_span` laws read only their spans, which is all those accessors return;
  /// the `front()`/`back()` reads below compare the whole entry, because that is what those two
  /// return and what a restore then resumes from (#183).
  fn assert_resident(&self, cache: &C, cap: usize, want: &[CachedTokenOf<'inp, L>], when: &str) {
    let name = self.label();
    assert!(
      cache.len() == want.len(),
      "tokora cache conformance [{name} exact-length] {when}: len() is {}, expected {}",
      cache.len(),
      want.len()
    );
    // `is_empty()` is a DEFAULT method (`len() == 0`) a fixture is free to override, and
    // `check_empty` (below) only ever calls it on a cache `len()` has already established is
    // empty — so a `true`-returning override, independent of `len`, was indistinguishable from a
    // correct one everywhere else this kit ran. `assert_resident` runs at every residency every
    // other check already sweeps — full, drained from the front, drained from the back — so
    // checking it HERE, against the same `want` every other observable in this function answers
    // for, asks the one question `check_empty` structurally cannot: whether `is_empty()` ever
    // says `true` while `want` is not.
    assert!(
      cache.is_empty() == want.is_empty(),
      "tokora cache conformance [{name} empty-invariants] {when}: is_empty() is {}, expected {} against {} resident entr(ies)",
      cache.is_empty(),
      want.is_empty(),
      want.len()
    );
    assert!(
      cache.remaining() == cap - want.len(),
      "tokora cache conformance [{name} exact-length] {when}: remaining() is {}, expected {} ({cap} capacity minus {} resident)",
      cache.remaining(),
      cap - want.len(),
      want.len()
    );
    // The two span accessors, against the spans of the entries the kit is tracking. These are
    // span-valued by signature — `front_span`/`back_span` return `&L::Span` and nothing else — so
    // the entry comparison has no purchase here; it belongs to the `front()`/`back()` reads below.
    let want_front_span = want.first().map(span_of::<L>);
    let want_back_span = want.last().map(span_of::<L>);
    match (want_front_span.as_ref(), cache.front_span()) {
      (Some(expected), Some(got)) => assert!(
        got == expected,
        "tokora cache conformance [{name} fifo-append] {when}: front is {got:?}, expected the OLDEST resident entry {expected:?}"
      ),
      (None, None) => {}
      (a, b) => panic!(
        "tokora cache conformance [{name} fifo-append] {when}: front presence disagrees — expected {a:?}, got {b:?}"
      ),
    }
    match (want_back_span.as_ref(), cache.back_span()) {
      (Some(expected), Some(got)) => assert!(
        got == expected,
        "tokora cache conformance [{name} fifo-append] {when}: back is {got:?}, expected the NEWEST resident entry {expected:?}. `push_back` appends after every resident entry."
      ),
      (None, None) => {}
      (a, b) => panic!(
        "tokora cache conformance [{name} fifo-append] {when}: back presence disagrees — expected {a:?}, got {b:?}"
      ),
    }
    // `front()` and `back()` name the same two entries `front_span()` and `back_span()` do.
    //
    // Outside the empty state the kit read those two accessors and nothing else, so `front`/`back`
    // were checked for PRESENCE (in `assert_empty`) and never for identity (#180 part A, item 8).
    // `front_span`/`back_span` are DEFAULT methods derived from `front`/`back`, so a cache that
    // overrides only `front` is already caught by the span check above — but `front_span` is
    // exactly the accessor a ring specializes off its head index rather than paying for a
    // `CachedTokenRef`, and a cache that overrides BOTH, with the cheap span half right and the
    // reference half wrong, had nothing to answer to. The entry a caller reads through `front()`
    // carries the token and the `L::State` a restore resumes from; the span half alone does not —
    // which is why since #183 these two reads compare the whole entry and not its span. A ring
    // that assembles its edge references out of a span array and a state array, one index apart,
    // answers the span half perfectly.
    self.assert_front_entry(cache, want.first(), when);
    self.assert_back_entry(cache, want.last(), when);
  }

  // ── 4. pop order ────────────────────────────────────────────────────────────────

  fn check_pop_order(&self, cap: usize) {
    if cap == 0 {
      return;
    }
    let name = self.label();

    // pop_front drains oldest-first, and `front` names the entry the next pop will return.
    let (mut cache, want) = self.filled(cap);
    for (i, expected) in want.iter().enumerate() {
      let viewed = cache
        .front_span()
        .unwrap_or_else(|| {
          panic!(
            "tokora cache conformance [{name} order]: front() is empty with {} entries still to pop",
            want.len() - i
          )
        })
        .clone();
      let popped = self.drained_front(
        &mut cache,
        expected,
        &format!("order pop_front #{i}"),
        format_args!(
          "tokora cache conformance [{name} order]: pop_front() is empty with {} entries still to pop",
          want.len() - i
        ),
        " `pop_front` removes the OLDEST resident entry.",
      );
      let got = span_of::<L>(&popped);
      assert!(
        viewed == got,
        "tokora cache conformance [{name} order]: front() viewed {viewed:?} but pop_front() removed {got:?}; they must name the same entry"
      );
    }
    // Presence-only, and the law: after a full drain there is no entry left for a pop to return,
    // so absence is the whole observable. (Documented exception — see `checked_push`.)
    assert!(
      // CACHE_CALL_CENSUS: absence
      cache.pop_front().is_none(),
      "tokora cache conformance [{name} order]: pop_front() still answers after every resident entry was drained"
    );

    // pop_back drains newest-first — the law the input layer's restore path is built on, since
    // pushes append to the back only and so the abandoned continuation's entries are the newest.
    let (mut cache, want) = self.filled(cap);
    for (i, expected) in want.iter().rev().enumerate() {
      let viewed = cache
        .back_span()
        .unwrap_or_else(|| {
          panic!("tokora cache conformance [{name} order]: back() is empty mid-drain")
        })
        .clone();
      let popped = self.drained_back(
        &mut cache,
        expected,
        &format!("order pop_back #{i}"),
        format_args!("tokora cache conformance [{name} order]: pop_back() is empty mid-drain"),
        " `pop_back` removes the NEWEST resident entry. The input layer drops an abandoned continuation's entries with a run of pop_back calls, so a pop_back that is not newest-first drops the wrong tokens.",
      );
      let got = span_of::<L>(&popped);
      assert!(
        viewed == got,
        "tokora cache conformance [{name} order]: back() viewed {viewed:?} but pop_back() removed {got:?}; they must name the same entry"
      );
    }
    // Presence-only, and the law — see the `pop_front` twin above.
    assert!(
      // CACHE_CALL_CENSUS: absence
      cache.pop_back().is_none(),
      "tokora cache conformance [{name} order]: pop_back() still answers after every resident entry was drained"
    );
  }

  // ── 5. push_front prepends ──────────────────────────────────────────────────────

  fn check_push_front(&self, cap: usize) {
    if cap == 0 {
      // Always full, so a front push is refused with a warrant and there is no residency for a
      // prepend to be observed against. Check 5's refusal law at capacity 0 is check 3's.
      return;
    }
    let name = self.label();
    let corpus = self.corpus(cap.saturating_add(1));
    let mut cache = self.make();
    let mut resident: Vec<CachedTokenOf<'inp, L>> = Vec::new();
    let mut refusals = 0usize;

    // One at the back, then everything else at the front: each must land BEFORE the rest.
    let mut it = corpus.into_iter();
    let first = it.next().expect("the corpus has at least two tokens");
    let want_first = self.accepted_push_back(
      &mut cache,
      first,
      "push_back seeding the prepend driver",
      format_args!(
        "tokora cache conformance [{name} push-front]: the first push_back into an empty cache was refused"
      ),
    );
    resident.push(want_first);

    for (i, tok) in it.enumerate() {
      let offered = tok.clone();
      let span = span_of::<L>(&tok);
      let full = resident.len() == cap;
      // The prepend arm's mixed site — see the `push_back` twin in check 3.
      let refusal = format!(
        "tokora cache conformance [{name} exact-length]: push_front #{i} was REFUSED with {} of {cap} slots used; a push is refused only when the cache is FULL. The input layer's put-back lands here: a refusal parks the token in the fallback slot (or panics outright, where RETAINS_FRONT is declared), so refusing early spends a resource the cache had no need of.",
        resident.len()
      );
      match self.push_front_expecting(
        &mut cache,
        tok,
        &format!("push_front #{i}"),
        !full,
        format_args!("{refusal}"),
      ) {
        Ok(placed) => {
          assert!(
            !full,
            "tokora cache conformance [{name} exact-length]: push_front #{i} was accepted at capacity {cap}"
          );
          resident.insert(0, placed);
          // The prepend law itself, before the generic residency check: the entry just pushed
          // must be the one `front` names.
          match cache.front_span() {
            Some(got) => assert!(
              *got == span,
              "tokora cache conformance [{name} push-front]: after push_front the front is {got:?}, expected the token just pushed, {span:?}. `push_front` places a token BEFORE every resident entry."
            ),
            None => panic!(
              "tokora cache conformance [{name} push-front]: front() is empty right after an accepted push_front"
            ),
          }
        }
        Err(returned) => {
          // A refusal has to be EARNED, exactly as on the `push_back` arm: a cache that hands
          // the token back with room still left has refused nothing it was entitled to refuse.
          // That assertion now lives in the router above, as the `refusal` message it panics
          // with when `must_accept` holds — same wording, but reached only *after* the returned
          // entry has been compared. Re-asserting `full` here would be an assertion that cannot
          // fail, since the router diverges on every refusal this arm would have caught.
          let back_span = span_of::<L>(&returned);
          self.assert_owned_entry_eq(
            &returned,
            &offered,
            "refusal-round-trip push_front",
            format_args!(
              "tokora cache conformance [{name} refusal-round-trip]: a refused push_front returned a DIFFERENT token: pushed {span:?}, got back {back_span:?}"
            ),
            NO_NOTE,
          );
          self.assert_resident(&cache, cap, &resident, "after a refused push_front");
          refusals += 1;
          continue;
        }
      }
      self.assert_resident(&cache, cap, &resident, "after push_front");
    }
    // Capacity 1 is the one capacity where the loop above is expected to accept nothing: the
    // seeding `push_back` fills the cache, so every front push after it meets a full one. This
    // check used to return at its first line for `cap < 2` and skip that capacity entirely
    // (#180 part A, item 10) — but "no prepend ORDER to observe" is not "nothing to observe".
    // What is left is the refusal half, and nothing else in the kit reaches it on this arm:
    // check 3 drives the round-trip for `push_back`, check 2 and `check_empty_push_front` only
    // ever drive front pushes that are ACCEPTED. So a cache that corrupts a refused front push —
    // hands back a resident entry and swallows the offered token — was invisible at capacity 1,
    // which is exactly the capacity where every front push is refused.
    if cap == 1 {
      assert!(
        refusals > 0,
        "tokora cache conformance kit bug [{name} push-front]: at capacity 1 the driver never reached a refused push_front, so the one half of check 5 this capacity can express was not observed"
      );
    } else {
      assert!(
        resident.len() > 1,
        "tokora cache conformance [{name} push-front]: no push_front was ever accepted at capacity {cap}, so the prepend law was never observed"
      );
    }

    // ── the whole sequence, not its two ends ──────────────────────────────────────
    //
    // Everything above reads the queue at its ENDS: `front` names the token just pushed,
    // `assert_resident` re-checks front and back, and `len`/`remaining` count. What lies BETWEEN
    // them is unobserved, and a `push_front` that lands its token at the front and permutes the
    // rest satisfies every one of those assertions — at capacity 4, `[d,b,c,a]` has the front
    // `push_front` promised, the back `push_back` left there, and four entries, and is not the
    // queue the law describes.
    //
    // So drain the sequence. A cache built to each accepted-prefix depth is drained with
    // `pop_front` and compared position by position: that reads the interior, and it reads it
    // through an operation whose own law check 4 has already established — on a residency built
    // the other way round, by `push_back` alone, so the oracle does not inherit the defect it is
    // looking for. A fresh cache per depth, since the drain consumes what it checks.
    for depth in 1..cap {
      let (mut mixed, order) = self.front_built(cap, depth);
      for (i, expected) in order.iter().enumerate() {
        let order_spans = Self::spans_of(&order);
        self.drained_front(
          &mut mixed,
          expected,
          &format!("push-front/full-order at depth {depth} position {i}"),
          format_args!(
            "tokora cache conformance [{name} push-front/full-order at depth {depth}]: pop_front() is empty at position {i} of the {} entries one push_back and {depth} push_front(s) put in",
            order.len()
          ),
          &format!(
            " After one push_back and {depth} push_front(s) the resident order is {order_spans:?} — the front pushes newest-first, then the token that went to the back. `push_front` places its token before every resident entry and MOVES NONE OF THEM; front, back and len do not say that between them, since a push_front that permutes the interior leaves all three right."
          ),
        );
      }
      // Presence-only, and the law: nothing is left to return. (Documented exception.)
      assert!(
        // CACHE_CALL_CENSUS: absence
        mixed.pop_front().is_none(),
        "tokora cache conformance [{name} push-front/full-order at depth {depth}]: pop_front() still answers after all {} entries were drained",
        order.len()
      );

      // ── and the same sequence read from the OTHER end ───────────────────────────
      //
      // The drain above reads a front-built residency with `pop_front` and nothing else, and
      // every OTHER `pop_back` in the kit — check 4's drain, check 6's and check 8's back
      // sweeps, the alternating drain check 6 runs across mutations — is driven against a cache
      // `filled` built with `push_back` from empty. So the two were never combined: a `pop_back`
      // that is correct on a back-built residency and wrong on one a `push_front` contributed to
      // had nothing to answer to (#180 part B, item 2). That is a ring whose prepend establishes
      // the new head and leaves the tail index naming a slot the prepend invalidated — and it is
      // the input layer's **restore** path that reads through it, after a put-back has prepended.
      //
      // A second cache at the same depth, since the drain above consumed the first.
      let (mut mixed, order) = self.front_built(cap, depth);
      for (i, expected) in order.iter().rev().enumerate() {
        let order_spans = Self::spans_of(&order);
        self.drained_back(
          &mut mixed,
          expected,
          &format!("push-front/full-order-from-the-back at depth {depth} pop_back #{i}"),
          format_args!(
            "tokora cache conformance [{name} push-front/full-order-from-the-back at depth {depth}]: pop_back() is empty at position {i} of the {} entries one push_back and {depth} push_front(s) put in",
            order.len()
          ),
          &format!(
            " After one push_back and {depth} push_front(s) the resident order is {order_spans:?}, so a newest-first drain must hand those back in reverse. Every other pop_back the kit drives runs against a cache built by push_back alone; this one runs against a residency a push_front built."
          ),
        );
      }
      // Presence-only, and the law: nothing is left to return. (Documented exception.)
      assert!(
        // CACHE_CALL_CENSUS: absence
        mixed.pop_back().is_none(),
        "tokora cache conformance [{name} push-front/full-order-from-the-back at depth {depth}]: pop_back() still answers after all {} entries were drained",
        order.len()
      );
    }

    // ── a build that INTERLEAVES the two arms ──────────────────────────────────────
    //
    // Every mixed residency above is the same shape: one `push_back`, and then every
    // `push_front`. So in the whole kit a `push_back` **never once followed** a `push_front` — the
    // seeding append is always the first operation, `filled` and `check_fifo_and_length` append
    // from empty and never prepend at all, and `check_clear`'s refill and `push_many` are appends
    // too (#180 part B, item 3). A cache whose append is right while the head has never moved and
    // wrong once a prepend has moved it — a slot computed from a stale head index — was
    // therefore driven by nothing.
    //
    // So alternate the arms, starting at the front, and read the whole sequence back. The order
    // is not the reverse of anything: `push_front`, `push_back`, `push_front`, `push_back` leaves
    // the prepends newest-first ahead of the appends oldest-first, which is a shape neither drain
    // above builds. And the length is swept rather than fixed, for the reason the depth above is:
    // a permutation of the interior needs a fourth resident entry before index 2 stops being the
    // tail, so a driver pinned at one short length puts the defect back where the contract and it
    // agree.
    for n in 2..=cap {
      let (mut mixed, order) = self.interleaved(cap, n);
      for (i, expected) in order.iter().enumerate() {
        let order_spans = Self::spans_of(&order);
        self.drained_front(
          &mut mixed,
          expected,
          &format!("push-front/interleaved-order at length {n} position {i}"),
          format_args!(
            "tokora cache conformance [{name} push-front/interleaved-order at length {n}]: pop_front() is empty at position {i} of the {} entries the alternating build put in",
            order.len()
          ),
          &format!(
            " Alternating push_front and push_back from empty, starting at the front, leaves the resident order {order_spans:?} — each prepend before every entry then resident, each append after every entry then resident. Neither arm may move an entry the other one placed."
          ),
        );
      }
      // Presence-only, and the law: nothing is left to return. (Documented exception.)
      assert!(
        // CACHE_CALL_CENSUS: absence
        mixed.pop_front().is_none(),
        "tokora cache conformance [{name} push-front/interleaved-order at length {n}]: pop_front() still answers after all {} entries were drained",
        order.len()
      );
    }
  }

  /// A fresh cache built by **alternating** the two push arms — `push_front`, `push_back`,
  /// `push_front`, … from empty — until `n` tokens are resident, together with the resident order
  /// it must be holding.
  ///
  /// The interleaving is the point: [`front_built`](Self::front_built) puts its single
  /// `push_back` first and every `push_front` after it, so nothing there — or anywhere else in
  /// the kit — ever appends to a queue a prepend has already touched.
  ///
  /// `n` is bounded by the capacity, so every push here has room and a refusal is a violation
  /// rather than a case to handle. Both arms' laws are already established before this runs:
  /// check 3 for the append, and — for the very first push, which goes to the front of an empty
  /// cache — [`check_empty_push_front`](Self::check_empty_push_front).
  fn interleaved(&self, cap: usize, n: usize) -> (C, Vec<CachedTokenOf<'inp, L>>) {
    let name = self.label();
    let corpus = self.corpus(n);
    assert!(
      corpus.len() == n,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but the kit needs {n} to build an alternating push_front/push_back residency at capacity {cap}. This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );

    let mut cache = self.make();
    let mut order: Vec<CachedTokenOf<'inp, L>> = Vec::new();
    for (i, tok) in corpus.into_iter().enumerate() {
      let to_front = i % 2 == 0;
      let arm = if to_front { "push_front" } else { "push_back" };
      let site = format!("{arm} #{i} building an interleaved residency");
      // Built as a `String` rather than held as `format_args!`: a `fmt::Arguments` bound to a
      // local borrows temporaries that are dropped at the end of that `let`, which the MSRV
      // (1.87) rejects even where a newer rustc's temporary-lifetime rules accept it. Passing it
      // through `format_args!("{refusal}")` keeps one copy of the wording for both arms.
      let refusal = format!(
        "tokora cache conformance [{name} push-front/interleaved-order at length {n}]: {arm} #{i} was REFUSED with {} of {cap} slots used; a push is refused only when the cache is FULL",
        order.len()
      );
      let placed = if to_front {
        self.accepted_push_front(&mut cache, tok, &site, format_args!("{refusal}"))
      } else {
        self.accepted_push_back(&mut cache, tok, &site, format_args!("{refusal}"))
      };
      if to_front {
        order.insert(0, placed);
      } else {
        order.push(placed);
      }
    }
    (cache, order)
  }

  /// The spans of a list of entries, for the messages that show the reader a whole sequence.
  ///
  /// The expectation lists are entries since #183; a message that printed them whole would have
  /// to `Debug` a token and a state per position, which buries the ordering it is trying to show.
  /// So the *sequence* messages stay span-valued and the *entry* messages
  /// ([`assert_entry_eq`](Self::assert_entry_eq)) name one component at a time.
  fn spans_of(entries: &[CachedTokenOf<'inp, L>]) -> Vec<L::Span> {
    entries.iter().map(span_of::<L>).collect()
  }

  /// A fresh cache in the shape [`check_push_front`](Self::check_push_front) builds — one
  /// `push_back`, then `depth` `push_front`s — together with the resident order it must be
  /// holding: the front-pushed tokens newest-first, then the one that went to the back.
  ///
  /// `depth` is bounded by `cap - 1`, so every push here has room and a refusal is a violation
  /// rather than a case to handle — the warranted-refusal law itself is checked by the caller,
  /// which drives one push past the capacity.
  fn front_built(&self, cap: usize, depth: usize) -> (C, Vec<CachedTokenOf<'inp, L>>) {
    let name = self.label();
    let want = depth.saturating_add(1);
    let corpus = self.corpus(want);
    assert!(
      corpus.len() == want,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but the kit needs {want} to build a cache of one push_back and {depth} push_front(s) at capacity {cap}. This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );

    let mut cache = self.make();
    let mut order: Vec<CachedTokenOf<'inp, L>> = Vec::new();
    let mut it = corpus.into_iter();
    let back = it.next().expect("the corpus was just checked non-empty");
    let want_back = self.accepted_push_back(
      &mut cache,
      back,
      &format!("push_back seeding a front-built residency at depth {depth}"),
      format_args!(
        "tokora cache conformance [{name} push-front/full-order at depth {depth}]: the seeding push_back into an empty cache was refused at capacity {cap}"
      ),
    );
    order.push(want_back);
    for (i, tok) in it.enumerate() {
      let placed = self.accepted_push_front(
        &mut cache,
        tok,
        &format!("push_front #{i} building a front-built residency at depth {depth}"),
        format_args!(
          "tokora cache conformance [{name} push-front/full-order at depth {depth}]: push_front #{i} was REFUSED with {} of {cap} slots used; a push is refused only when the cache is FULL",
          order.len()
        ),
      );
      order.insert(0, placed);
    }
    (cache, order)
  }

  // ── 6. bounded, pure peek ───────────────────────────────────────────────────────

  /// Check 6 at **every residency**, reached from **both ends**, deepest first.
  ///
  /// [`filled`](Self::filled) builds a cache at `len == capacity`, and that is the one residency
  /// where a `peek` bounded by the cache's BACKING store and a `peek` bounded by its live run are
  /// the same number. A fixed-array or ring implementation that reads its bound off the array
  /// rather than off its own `len` serves entries it has already handed out — and only a partly
  /// drained cache can say so. (A cache that copies through an iterator is immune by
  /// construction, since `take(n)` saturates at the length; that is exactly why every built-in
  /// passes either way, and why driving only the full cache leaves the law untested.)
  ///
  /// The shallower states are reached the way a parse reaches them, by popping a cache that was
  /// full — including the emptied one, where a `peek` still holding its old run answers with
  /// tokens that are nobody's lookahead any more. The full cache runs first, so a cache broken at
  /// every residency is still diagnosed in the simplest one.
  ///
  /// And the pops are driven from **both ends**, because the two leave stale entries in different
  /// places and a sweep that only drains the front asks about one of them. `pop_front` is the
  /// consuming path; `pop_back` is the **restore** path — the input layer drops an abandoned
  /// continuation's entries with a run of `pop_back` calls after `reconcile_cache_geometry`, so a
  /// cache whose removed entries stay readable at the TAIL is corrupted on a live rollback rather
  /// than in a shape nothing drives. The two sweeps are asymmetric on the oracle, not on the
  /// checks: draining the front leaves the resident suffix `filled[popped..]`, draining the back
  /// the resident prefix `filled[..cap - popped]`, and each state then runs the same
  /// [`check_peek_at`](Self::check_peek_at) body — bound, order, purity, `peek_one` and the whole
  /// prefill-depth sweep. The back sweep starts at one pop, since popping nothing is the full
  /// cache the front sweep has already driven.
  fn check_peek(&self, cap: usize) {
    if cap == 0 {
      return;
    }
    let name = self.label();

    // Drained from the FRONT: the consuming path, leaving the resident suffix.
    for popped in 0..=cap {
      let (mut cache, filled) = self.filled_to_capacity(cap);
      for (i, expected) in filled.iter().take(popped).enumerate() {
        // The kit knows which entry each of these removes — `filled[i]`, oldest first — so the
        // pop is compared rather than counted, even though its only job here is to reach a
        // residency.
        self.drained_front(
          &mut cache,
          expected,
          &format!("bounded-peek/front-sweep pop_front #{i}"),
          format_args!(
            "tokora cache conformance [{name} bounded-peek]: pop_front() answered None after {i} of {popped} pops off a cache filled to its capacity {cap}"
          ),
          " These pops only exist to reach a residency, which is exactly why the entry they return was thrown away before #183.",
        );
      }
      let want = &filled[popped..];
      let state = if popped == 0 {
        format!("a full cache: {} of {cap} resident", want.len())
      } else {
        format!(
          "a partly drained cache: {} of {cap} resident, after {popped} pop_front(s) off a full one",
          want.len()
        )
      };
      self.check_peek_at(&cache, cap, want, &state);
    }

    // Drained from the BACK: the restore path, leaving the resident prefix. A cache that leaves
    // the entries `pop_back` handed out readable — a ring whose tail index moves back over slots
    // it does not clear — answers this sweep with lookahead the rollback just abandoned, and is
    // indistinguishable from a conforming cache in the front sweep above, where the stale slots
    // sit past a live run the bound saturates on first.
    for popped in 1..=cap {
      let (mut cache, filled) = self.filled_to_capacity(cap);
      for i in 0..popped {
        // Newest first, so the i-th removes `filled[cap - 1 - i]`. This is the restore path, so
        // the state message carries the restore note.
        self.drained_back(
          &mut cache,
          &filled[cap - 1 - i],
          &format!("bounded-peek/back-sweep pop_back #{i}"),
          format_args!(
            "tokora cache conformance [{name} bounded-peek]: pop_back() answered None after {i} of {popped} pops off a cache filled to its capacity {cap}"
          ),
          " These pops only exist to reach a residency.",
        );
      }
      let want = &filled[..cap - popped];
      let state = format!(
        "a partly drained cache: {} of {cap} resident, after {popped} pop_back(s) off a full one — the input layer's restore path",
        want.len()
      );
      self.check_peek_at(&cache, cap, want, &state);
    }
  }

  /// Check 6 against **one cache instance**, peeked → mutated → peeked again, all the way down.
  ///
  /// [`check_peek`](Self::check_peek) reaches every residency, but it reaches each of them on a
  /// cache built **fresh** for it: `filled_to_capacity`, then the pops, and only then the first
  /// peek. So every `peek` the kit ever made was the first one that instance had answered at that
  /// residency, and a `peek` that memoises its first answer — and keeps serving it after later
  /// pops — was byte-for-byte a conforming one everywhere the kit looked (#180 part B, item 1).
  /// It even satisfies the purity law, which is the only repeatability the kit asked for: two
  /// peeks with nothing changed in between DO agree; the latch is only wrong once something has
  /// changed.
  ///
  /// So this drives the missing shape and nothing else: one instance, filled to capacity, peeked
  /// in full, then popped and peeked again at every residency down to empty. The pops alternate
  /// ends — a latch is equally stale after either, and a cache that invalidates on one path and
  /// not the other is a real shape (the input layer's rollback pops the back, its consume path
  /// the front). Everything each state is then checked for is [`check_peek_at`](Self::check_peek_at)'s
  /// whole body, unchanged, so this adds a driver rather than an oracle.
  ///
  /// The pops themselves are an oracle, though, and are the one thing here that is not
  /// `check_peek_at`'s: each popped **entry** is compared in full before `want` is shortened.
  /// Every other drain in the kit runs on an instance that has never been peeked, so this is the
  /// only place a `peek` that leaves state behind can be caught poisoning a later pop — see the
  /// comment on the comparison itself.
  fn check_peek_across_mutations(&self, cap: usize) {
    if cap == 0 {
      return;
    }
    let name = self.label();
    let (mut cache, filled) = self.filled_to_capacity(cap);
    let mut want: Vec<CachedTokenOf<'inp, L>> = filled;
    self.check_peek_at(
      &cache,
      cap,
      &want,
      &format!("ONE cache instance, full at {cap}, before anything has mutated it"),
    );

    let mut pops = 0usize;
    let mut from_front = true;
    while !want.is_empty() {
      let end = if from_front { "pop_front" } else { "pop_back" };
      // The popped ENTRY, not merely its presence.
      //
      // This is the only drain in the kit that runs on an instance the kit has **already
      // peeked**, and until #183's review it read nothing but `is_some()` — the value was bound,
      // discarded, and `want` was shortened blindly. So a cache whose `peek` leaves state behind
      // (a cached lookahead frontier, a memoised run) and whose later pops report out of it was
      // invisible here: it removes the right entry, so the residency stays right; it returns the
      // right span, so every span-level law stays right; and the token and the `L::State` the
      // caller actually resumes from come from a different position. That is the #183 defect
      // class on the peek-then-consume path — which is the path the issue's severity argument
      // rests on, since `InputRef::resume` is lookahead followed by a rebuild — and no mutant on
      // any return path could reach it while the value was thrown away.
      let expected = if from_front {
        want.first().expect("the loop guard says want is non-empty")
      } else {
        want.last().expect("the loop guard says want is non-empty")
      };
      let which = if from_front { "OLDEST" } else { "NEWEST" };
      let site = format!("bounded-peek/across-mutations {end}() #{pops}");
      let absent = format_args!(
        "tokora cache conformance [{name} bounded-peek]: {end}() answered None with {} of {cap} entries still resident",
        want.len()
      );
      let tail =
        format!(" It is the {which} resident entry, off an instance this kit has ALREADY peeked.");
      if from_front {
        self.drained_front(&mut cache, expected, &site, absent, &tail);
      } else {
        self.drained_back(&mut cache, expected, &site, absent, &tail);
      }
      if from_front {
        want.remove(0);
      } else {
        want.pop();
      }
      pops += 1;
      from_front = !from_front;
      self.check_peek_at(
        &cache,
        cap,
        &want,
        &format!(
          "the SAME cache instance this kit has ALREADY peeked, after {pops} pop(s) — the last a {end}() — leaving {} of {cap} resident",
          want.len()
        ),
      );
    }
  }

  /// [`filled`](Self::filled) at exactly the capacity, which is what both residency sweeps in
  /// [`check_peek`](Self::check_peek) read their expectation off: each slices that list, so it has
  /// to be the full residency before the first pop.
  ///
  /// Check 3 drives the same fill one token further and would have failed already; this says so
  /// in the kit's own words rather than as a slice index panic.
  fn filled_to_capacity(&self, cap: usize) -> (C, Vec<CachedTokenOf<'inp, L>>) {
    let name = self.label();
    let (cache, filled) = self.filled(cap);
    assert!(
      filled.len() == cap,
      "tokora cache conformance [{name} bounded-peek]: filling a fresh cache with {cap} token(s) left {} of them resident",
      filled.len()
    );
    (cache, filled)
  }

  /// Check 6 in full — bound, order, purity, `peek_one`, and the prefilled-buffer sweep —
  /// against a cache holding exactly `want`. `state` names the residency in every message.
  fn check_peek_at(&self, cache: &C, cap: usize, want: &[CachedTokenOf<'inp, L>], state: &str) {
    let name = self.label();
    let window = <<PeekWindow as Window>::CAPACITY as Unsigned>::USIZE;
    let bound = window.min(want.len());
    let want_spans = Self::spans_of(want);

    let first = self.peeked_entries(cache);
    assert!(
      first.len() == bound,
      "tokora cache conformance [{name} bounded-peek] against {state}: peek() appended {} entries into an empty {window}-slot buffer, expected exactly min(len, the buffer's remaining capacity) = {bound}. The bound is the cache's CURRENT length, not the room its backing store has: an entry that has been popped is not lookahead any more.",
      first.len()
    );
    // This one assertion carries three of check 6's clauses at once: the ORDER (oldest first),
    // the CONTENT (these entries and no others), and "**each resident token once**". The third
    // has no assertion of its own and does not get one: `corpus` lexes successive tokens, so the
    // spans in `want` are pairwise distinct at every capacity and from every source, and an
    // appended run that equals `want[..bound]` is therefore distinct by construction. A separate
    // distinctness check could not be made to fail while this one passes, and an assertion that
    // cannot fail is not a check (#180 part A, item 9). What the clause was missing is a fixture
    // that violates it and nothing else, which `DUPLICATING_PEEK` in `cache_tests.rs` now is: the
    // right count, the right first entry, the front served `bound` times over.
    let first_spans: Vec<L::Span> = first.iter().map(|e| e.0.clone()).collect();
    assert!(
      first_spans == want_spans[..bound],
      "tokora cache conformance [{name} bounded-peek] against {state}: peek() appended {first_spans:?}, expected the resident prefix OLDEST FIRST {:?}",
      &want_spans[..bound]
    );
    // …and the other two thirds of every entry the run just landed. The vector comparison above
    // is the ORDER law and keeps its own wording; this is the entry law (#183), and it is what
    // separates a `peek` that serves the right run from one that serves the right spans with
    // re-associated tokens or states behind them.
    self.assert_peeked_run_eq(
      &first,
      &want[..bound],
      &format!("bounded-peek against {state}"),
    );

    // Purity: the cache is unchanged, and a second peek reads the same.
    self.assert_resident(cache, cap, want, &format!("after peek() against {state}"));
    let second = self.peeked_entries(cache);
    assert!(
      second == first,
      "tokora cache conformance [{name} pure-peek] against {state}: two peeks on an unchanged cache disagreed: {first:?} then {second:?}. `peek` takes &self and must be logically pure. The comparison is over whole (span, token, state) entries, so a peek that is stable in its spans and unstable in the tokens or the states behind them disagrees here too."
    );

    // `peek_one` is the single-slot case: it names the front, and it names nothing where a
    // drained cache has no front left to name.
    //
    // Taken as an owned span rather than matched in place, so the borrow ends and the SECOND
    // call below can be made against the same cache. Until it was, `peek_one` was called exactly
    // once per residency this sweep visits — against a cache instance built fresh for that
    // residency — so a `peek_one` that is correct once and wrong on every repeat had nothing to
    // disagree with. `peek` has been held to that since the kit existed, three lines above; this
    // is the same law on the method composed from it (#180 part A, item 4).
    // Taken as an owned TRIPLE rather than an owned span, for the same reason the run above is:
    // `peek_one` names an entry, and naming the right span while carrying somebody else's state
    // is exactly the shape #183 exists to reject. The purity comparison below then covers all
    // three components too.
    let one_first: Option<PeekedTriple<'inp, L>> = self.peeked_one(cache);
    match (&one_first, want.first()) {
      (Some(got), Some(expected)) => self.assert_peeked_entry_eq(
        got,
        expected,
        &format!("bounded-peek peek_one() against {state}"),
        format_args!(
          "tokora cache conformance [{name} bounded-peek] against {state}: peek_one() named {:?}, expected the front entry {:?}",
          got.0,
          span_of::<L>(expected)
        ),
      ),
      (Some(got), None) => panic!(
        "tokora cache conformance [{name} bounded-peek] against {state}: peek_one() answered {:?} with NOTHING resident",
        got.0
      ),
      (None, Some(_)) => panic!(
        "tokora cache conformance [{name} bounded-peek] against {state}: peek_one() is empty with {} entries resident",
        want.len()
      ),
      (None, None) => {}
    }
    let one_second: Option<PeekedTriple<'inp, L>> = self.peeked_one(cache);
    assert!(
      one_second == one_first,
      "tokora cache conformance [{name} pure-peek-one] against {state}: two peek_one() calls on an unchanged cache disagreed: {one_first:?} then {one_second:?}. `peek_one` takes &self and must be logically pure, exactly as `peek` must."
    );

    // ── the same law against a buffer that is NOT empty ───────────────────────────
    //
    // `peeked_spans` above always starts from a fresh, empty buffer, where the buffer's
    // remaining capacity and its total capacity are the same number. That shape is real too —
    // `InputRef`'s peek fill (`peek/mod.rs`) hands `cache.peek` an empty `buf` on a cache hit or
    // an overflow-free fill, whenever nothing is parked — but it is not the only one: on the
    // paths that reach the call with a token parked, or with lexed overflow staged ahead of it,
    // the peek fill has already pushed that entry before it calls in, so `buf` can already hold
    // entries by the time `peek` sees it, with less room left than its total. Re-run the law in
    // that shape too.
    //
    // What this shape buys, precisely, is the APPEND half: `peek` writes behind what is already
    // in the buffer and leaves it alone. Against an empty buffer a `peek` that clears it first,
    // or that treats it as an output slot to fill rather than a queue to extend, is
    // indistinguishable from a correct one; here it is not.
    //
    // What it does NOT buy — stated here because the arithmetic is not obvious and the module
    // docs' "does not check" section owes the reader the same warning at the top of the file: a
    // `peek` bounded by the buffer's TOTAL capacity that pushes the oldest entries in order and
    // silently discards what `push_back` refuses lands exactly the entries a correct one would,
    // in every configuration. `min(min(len, W), W - P) == min(len, W - P)`, and a refused
    // `GenericArrayDeque::push_back` returns the value and leaves the deque untouched. That
    // defect has no observable consequence through the `Cache` surface at all, so no assertion
    // below is aimed at it.
    //
    // The prefill tokens are deliberately ones the cache is NOT holding. Prefilling with a token
    // the cache also has resident would let a `peek` that cleared the buffer and re-appended its
    // own run satisfy the untouched-prefix assertion by coincidence, since the buffer's first
    // entry and the cache's front would carry the same span.
    //
    // And the depth is swept rather than fixed. One prefilled entry is the shallowest buffer
    // that is not empty, so it separates append from overwrite — but nothing more: at depth 1
    // there is no prefix ORDER to disturb, the room left is the window all but one slot, and a
    // `peek` keyed on how much the buffer already holds reads the same on that call as a
    // conforming one. Every depth the window can hold is driven, up to and including the one
    // that leaves NO room at all, which is the only shape where the bound is asked to come out
    // zero.
    for depth in 1..=window {
      self.check_prefilled_peek(cache, cap, want, depth, Prefill::Later, state);
    }

    // ── and the same law through a window that is NOT the kit's own ───────────────
    //
    // Everything above peeks through [`PeekWindow`], and until this ran, so did every other
    // driver in the kit: `W` was `U4` in all of them (#180 part B, item 4). Sweeping the prefill
    // depth does not stand in for varying it — that moves the buffer's REMAINING capacity, and
    // `peek` is generic over `W`, can read `W::CAPACITY` off the type, and is free to branch on
    // it. Two shapes of that branch are worth naming, and both are dead code at a single window:
    // the single-slot fast path, which the trait itself invites (`peek_one`'s default body is
    // `peek::<U1>` into a one-slot buffer), and the truncating path a cache takes when the
    // residency does NOT fit in the window — which never runs while the window is as wide as
    // every residency the kit builds.
    self.check_peek_window::<PeekWindowOne>(cache, want, state);
    self.check_peek_window::<PeekWindowMid>(cache, want, state);
  }

  /// Check 6's bound, order and purity through **one** window type, against a cache holding
  /// exactly `want`.
  ///
  /// Only the empty-buffer shape: the prefilled sweep above already varies the room left, at
  /// [`PeekWindow`], and what this adds is the other axis — the value of `W` itself. `peek_one`
  /// is not re-driven either; it takes no window.
  fn check_peek_window<W>(&self, cache: &C, want: &[CachedTokenOf<'inp, L>], state: &str)
  where
    W: Window,
  {
    let name = self.label();
    let window = <<W as Window>::CAPACITY as Unsigned>::USIZE;
    let bound = window.min(want.len());
    let want_spans = Self::spans_of(want);

    let first = self.peeked_entries_through::<W>(cache);
    assert!(
      first.len() == bound,
      "tokora cache conformance [{name} bounded-peek/window {window}] against {state}: peek() appended {} entries into an empty {window}-slot buffer, expected exactly min(len, the buffer's remaining capacity) = {bound}. `peek` is generic over W and this is a window the kit's own PeekWindow is not: the bound is the same law at every one of them.",
      first.len()
    );
    let first_spans: Vec<L::Span> = first.iter().map(|e| e.0.clone()).collect();
    assert!(
      first_spans == want_spans[..bound],
      "tokora cache conformance [{name} bounded-peek/window {window}] against {state}: peek() appended {first_spans:?}, expected the resident prefix OLDEST FIRST {:?}. A cache may not read W::CAPACITY and answer differently for it.",
      &want_spans[..bound]
    );
    self.assert_peeked_run_eq(
      &first,
      &want[..bound],
      &format!("bounded-peek/window {window} against {state}"),
    );
    let second = self.peeked_entries_through::<W>(cache);
    assert!(
      second == first,
      "tokora cache conformance [{name} pure-peek/window {window}] against {state}: two peeks on an unchanged cache disagreed: {first:?} then {second:?}. `peek` takes &self and must be logically pure at every window, not only at the kit's own."
    );
  }

  /// Check 6's prefilled half at one prefill depth: `peek` into a buffer that already holds
  /// `depth` entries — tokens the cache is not itself resident on — with `window - depth` slots
  /// left behind them.
  ///
  /// [`check_peek_at`](Self::check_peek_at) runs this at **every** depth from one entry to a full
  /// buffer, and the sweep is the point: a single depth puts the driver back where a defect and
  /// the contract agree. At depth 1 a `peek` that clobbers, reorders or miscounts only behind a
  /// deeper prefix is byte-for-byte a conforming one; below the top depth the room left is never
  /// zero, so a `peek` that ignores a full buffer is never asked the question. `state` names the
  /// residency it is driven at, which [`check_peek`](Self::check_peek) sweeps in its turn.
  fn check_prefilled_peek(
    &self,
    cache: &C,
    cap: usize,
    want: &[CachedTokenOf<'inp, L>],
    depth: usize,
    from: Prefill,
    state: &str,
  ) {
    let name = self.label();
    let window = <<PeekWindow as Window>::CAPACITY as Unsigned>::USIZE;
    let tag = from.tag();
    let prefill = self.prefill(cap, depth, from);
    // Cloned BEFORE the entries move into the buffer: the "peek left the prefix untouched"
    // assertion compares whole entries since #183, so the expectation has to be the whole entry
    // and not the span projection the kit kept before.
    let prefill_want: Vec<CachedTokenOf<'inp, L>> = prefill.clone();
    let prefill_spans = Self::spans_of(&prefill_want);
    let want_spans = Self::spans_of(want);
    let remaining_at_prefill = window - depth;
    let prefilled_bound = remaining_at_prefill.min(want.len());
    let (prefix_after, appended) = self.peeked_entries_after_prefill(cache, prefill);
    let prefix_spans: Vec<L::Span> = prefix_after.iter().map(|e| e.0.clone()).collect();
    assert!(
      prefix_spans == prefill_spans,
      "tokora cache conformance [{name} bounded-peek/{tag} at depth {depth}] against {state}: peek() changed the {depth} entr(ies) already in the buffer ahead of it — got {prefix_spans:?}, expected them untouched, {prefill_spans:?}. `peek` appends BEHIND what the buffer already holds; it neither overwrites nor reorders it."
    );
    self.assert_peeked_run_eq(
      &prefix_after,
      &prefill_want,
      &format!("bounded-peek/{tag} at depth {depth} untouched prefix against {state}"),
    );
    assert!(
      appended.len() == prefilled_bound,
      "tokora cache conformance [{name} bounded-peek/{tag} at depth {depth}] against {state}: peek() appended {} entries into a buffer already holding {depth} of {window} slots, expected exactly min(len, buffer's REMAINING capacity) = {prefilled_bound}.",
      appended.len()
    );
    let appended_spans: Vec<L::Span> = appended.iter().map(|e| e.0.clone()).collect();
    assert!(
      appended_spans == want_spans[..prefilled_bound],
      "tokora cache conformance [{name} bounded-peek/{tag} at depth {depth}] against {state}: peek() appended {appended_spans:?}, expected the resident prefix OLDEST FIRST {:?}. Every resident entry is served, whatever the buffer already holds: `peek` is not entitled to decide the caller has one of them already.",
      &want_spans[..prefilled_bound]
    );
    self.assert_peeked_run_eq(
      &appended,
      &want[..prefilled_bound],
      &format!("bounded-peek/{tag} at depth {depth} against {state}"),
    );

    // Purity on THIS path too. The prefilled buffer is a different route through a cache's
    // `peek` than the empty one — a cache with interior mutability can be inert on the branch
    // the empty buffer takes and drain, reorder or re-time its residency on the branch a
    // non-empty one takes. Nothing above reuses the cache after the prefilled call, so the
    // damage would go unseen: check the cache is still what it was, and that a second prefilled
    // peek over a fresh prefill reads identically.
    self.assert_resident(
      cache,
      cap,
      want,
      &format!("after prefilled peek() at depth {depth} against {state}"),
    );
    let (prefix_again, appended_again) =
      self.peeked_entries_after_prefill(cache, self.prefill(cap, depth, from));
    assert!(
      prefix_again == prefix_after && appended_again == appended,
      "tokora cache conformance [{name} pure-peek/{tag} at depth {depth}] against {state}: two prefilled peeks on an unchanged cache disagreed: prefix {prefix_after:?} then {prefix_again:?}, appended {appended:?} then {appended_again:?}. `peek` takes &self and must be logically pure against every shape of buffer, not only against an empty one."
    );
    self.assert_resident(
      cache,
      cap,
      want,
      &format!("after a second prefilled peek() at depth {depth} against {state}"),
    );
  }

  /// The `depth` prefill entries for one of the two arrangements — see [`Prefill`].
  fn prefill(&self, cap: usize, depth: usize, from: Prefill) -> Vec<CachedTokenOf<'inp, L>> {
    match from {
      Prefill::Later => self.beyond_residency(cap, depth),
      Prefill::Earlier => self.before_residency(depth),
    }
  }

  /// The **first** `depth` corpus tokens — spans that precede every entry of the residency
  /// [`check_peek_behind_earlier_entries`](Self::check_peek_behind_earlier_entries) builds, which
  /// starts a full peek window into the corpus for exactly that reason.
  ///
  /// This is the arrangement the real call site produces, and the one no driver in this kit ever
  /// built (#180 part B, item 6).
  fn before_residency(&self, depth: usize) -> Vec<CachedTokenOf<'inp, L>> {
    let name = self.label();
    let corpus = self.corpus(depth);
    assert!(
      corpus.len() == depth,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but the kit needs {depth} of them BEFORE the residency for a peek-buffer prefill whose spans precede it. This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );
    corpus
  }

  /// `depth` consecutive corpus tokens the cache under test is **not** holding: corpus indices
  /// `cap..cap + depth`, starting one past the residency [`filled`](Self::filled) builds at that
  /// capacity.
  ///
  /// [`check_peek`](Self::check_peek) prefills the peek buffer with these rather than with tokens
  /// the cache also holds, so that "`peek` left the entries already in the buffer alone" is an
  /// assertion a `peek` which cleared the buffer and re-appended its own run cannot satisfy by
  /// coincidence. They are handed over in corpus order, so at `depth > 1` the prefix has an
  /// order of its own for `peek` to disturb. Every residency the sweep drives is a contiguous run
  /// of `filled`'s — a suffix where it drained the front, a prefix where it drained the back — so
  /// these stay non-resident at all of them.
  fn beyond_residency(&self, cap: usize, depth: usize) -> Vec<CachedTokenOf<'inp, L>> {
    let name = self.label();
    let want = cap.saturating_add(depth);
    let mut corpus = self.corpus(want);
    assert!(
      corpus.len() == want,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but the kit needs {want} — {depth} past the capacity {cap} — for a peek-buffer prefill of {depth} entr(ies) the cache is not itself holding. This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );
    corpus.split_off(cap)
  }

  /// One peeked entry as the triple the kit compares, arm-blind: the span and the token through
  /// [`PeekedTokenExt`], the state through [`peeked_state`].
  fn triple(entry: &MaybeRefCachedTokenOf<'_, 'inp, L>) -> PeekedTriple<'inp, L> {
    (
      entry.span().clone(),
      entry.token().clone(),
      peeked_state::<L>(entry).clone(),
    )
  }

  /// The entries `peek` appends, in order, into a fresh, empty buffer of the kit's own window.
  fn peeked_entries(&self, cache: &C) -> Vec<PeekedTriple<'inp, L>> {
    self.peeked_entries_through::<PeekWindow>(cache)
  }

  /// [`peeked_entries`](Self::peeked_entries) through an arbitrary window, which is what
  /// [`check_peek_window`](Self::check_peek_window) needs and what the kit had no way to express
  /// while every buffer it built was a [`PeekWindow`] one.
  fn peeked_entries_through<W>(&self, cache: &C) -> Vec<PeekedTriple<'inp, L>>
  where
    W: Window,
  {
    let mut buf: GenericArrayDeque<MaybeRefCachedTokenOf<'_, 'inp, L>, W::CAPACITY> =
      GenericArrayDeque::new();
    // CACHE_CALL_CENSUS: routed
    cache.peek::<W>(&mut buf);
    buf.iter().map(Self::triple).collect()
  }

  /// The prefilled-buffer counterpart of [`peeked_entries`](Self::peeked_entries): loads
  /// `prefill` into the buffer first, calls `peek`, then splits the result into the entries
  /// standing where the original prefix was (to check `peek` left it alone) and the entries
  /// `peek` appended behind it, in order. This is the other shape the real call site hands
  /// `Cache::peek` — already holding a parked token or staged overflow, where
  /// [`peeked_entries`](Self::peeked_entries) is the empty-`buf` shape a cache hit or an
  /// overflow-free fill hands over — see [`check_peek`](Self::check_peek) for what that shape
  /// catches, and for the one thing it provably cannot.
  fn peeked_entries_after_prefill(
    &self,
    cache: &C,
    prefill: Vec<CachedTokenOf<'inp, L>>,
  ) -> (Vec<PeekedTriple<'inp, L>>, Vec<PeekedTriple<'inp, L>>) {
    let prefill_len = prefill.len();
    let mut buf: GenericArrayDeque<
      MaybeRefCachedTokenOf<'_, 'inp, L>,
      <PeekWindow as Window>::CAPACITY,
    > = GenericArrayDeque::new();
    for tok in prefill {
      assert!(
        // CACHE_CALL_CENSUS: not-a-cache-call
        buf.push_back(Maybe::Owned(tok)).is_none(),
        "tokora cache conformance kit bug: its own prefill overflowed the peek window before `peek` was even called"
      );
    }
    // CACHE_CALL_CENSUS: routed
    cache.peek::<PeekWindow>(&mut buf);
    let mut entries = buf.iter().map(Self::triple);
    let prefix = entries.by_ref().take(prefill_len).collect();
    let appended = entries.collect();
    (prefix, appended)
  }

  // ── 7. clear ────────────────────────────────────────────────────────────────────

  fn check_clear(&self, cap: usize) {
    let (mut cache, _) = self.filled(cap);
    cache.clear();
    self.assert_empty(&mut cache, cap, "after clear()");

    // The cache is never touched again after `clear()` without this: a `clear` that leaves it
    // permanently unable to accept a push — poisoned, its capacity zeroed, disabled outright —
    // passes everything above, since nothing above tries to use it again (#180 part A, item 2).
    // Reuse it: push back `cap` tokens and confirm they land exactly the way a first fill would.
    let name = self.label();
    let mut want = Vec::with_capacity(cap);
    for (i, tok) in self.corpus(cap).into_iter().enumerate() {
      let placed = self.accepted_push_back(
        &mut cache,
        tok,
        &format!("push_back #{i} refilling a cleared cache"),
        format_args!(
          "tokora cache conformance [{name} clear]: push_back() was refused while refilling a cache clear() just emptied — clear() must not leave the cache permanently unable to accept pushes"
        ),
      );
      want.push(placed);
    }
    self.assert_resident(
      &cache,
      cap,
      &want,
      "after refilling a cache clear() just emptied",
    );
  }

  // ── 8. the combined span ────────────────────────────────────────────────────────

  /// Sweeps every residency [`check_peek`](Self::check_peek) does — a full cache, and partially
  /// drained from both the front and the back — rather than checking `span()` once against a
  /// freshly filled cache and never again. A `span()` that is correct only immediately after a
  /// fill and stale after any pop (#180 part A, item 3) passed the single-residency check the
  /// same way [`STALE_RESIDENCY_PEEK`] passed `peek`'s: the one residency driven was the one
  /// residency the defect cannot be told apart from a conforming cache at.
  fn check_span(&self, cap: usize) {
    if cap == 0 {
      return;
    }
    let name = self.label();

    // Drained from the FRONT: the consuming path, leaving the resident suffix.
    for popped in 0..=cap {
      let (mut cache, filled) = self.filled_to_capacity(cap);
      for (i, expected) in filled.iter().take(popped).enumerate() {
        self.drained_front(
          &mut cache,
          expected,
          &format!("combined-span/front-sweep pop_front #{i}"),
          format_args!(
            "tokora cache conformance [{name} combined-span]: pop_front() answered None after {i} of {popped} pops off a cache filled to its capacity {cap}"
          ),
          " These pops only exist to reach a residency.",
        );
      }
      let want = &filled[popped..];
      let state = if popped == 0 {
        format!("a full cache: {} of {cap} resident", want.len())
      } else {
        format!(
          "a partly drained cache: {} of {cap} resident, after {popped} pop_front(s) off a full one",
          want.len()
        )
      };
      self.assert_span_at(&cache, &Self::spans_of(want), &state);
    }

    // Drained from the BACK: the restore path, leaving the resident prefix.
    for popped in 1..=cap {
      let (mut cache, filled) = self.filled_to_capacity(cap);
      for i in 0..popped {
        self.drained_back(
          &mut cache,
          &filled[cap - 1 - i],
          &format!("combined-span/back-sweep pop_back #{i}"),
          format_args!(
            "tokora cache conformance [{name} combined-span]: pop_back() answered None after {i} of {popped} pops off a cache filled to its capacity {cap}"
          ),
          " These pops only exist to reach a residency.",
        );
      }
      let want = &filled[..cap - popped];
      let state = format!(
        "a partly drained cache: {} of {cap} resident, after {popped} pop_back(s) off a full one — the input layer's restore path",
        want.len()
      );
      self.assert_span_at(&cache, &Self::spans_of(want), &state);
    }
  }

  /// Check 8 against a cache holding exactly `want`: `span()` runs from the front entry's start
  /// to the back entry's end, and is absent exactly when nothing is resident (the latter is
  /// also [`assert_empty`](Self::assert_empty)'s concern; asserted here too so every residency
  /// [`check_span`](Self::check_span) visits — including the fully drained one — is covered by
  /// the same oracle instead of splitting the empty case out).
  ///
  /// **The one oracle #183 left span-valued, and deliberately.** `Cache::span` returns an
  /// `Option<L::Span>` — a span *synthesized* from two endpoints, with no entry behind it — so
  /// there is no token and no state to compare. `want` is therefore a span list here where every
  /// other oracle takes entries; the callers project with [`spans_of`](Self::spans_of).
  fn assert_span_at(&self, cache: &C, want: &[L::Span], state: &str) {
    let name = self.label();
    match (want.first(), want.last(), cache.span()) {
      (Some(first), Some(last), Some(combined)) => assert!(
        combined.start_ref() == first.start_ref() && combined.end_ref() == last.end_ref(),
        "tokora cache conformance [{name} combined-span] against {state}: span() is {combined:?}, expected {:?}..{:?} — the front entry's start to the back entry's end",
        first.start_ref(),
        last.end_ref()
      ),
      (None, None, None) => {}
      (_, _, got) => panic!(
        "tokora cache conformance [{name} combined-span] against {state}: span() presence disagrees with the {} resident entr(ies) it should be built from — got {got:?}",
        want.len()
      ),
    }
  }

  // ── check 6 where the buffer's entries come BEFORE the residency ────────────────

  /// Check 6's prefilled half with the span relation between the buffer and the residency
  /// **reversed** — every entry already in the buffer precedes every resident one.
  ///
  /// Until this ran, that relation was one fixed value in the whole kit: the prefill came from
  /// [`beyond_residency`](Self::beyond_residency), corpus tokens past the residency, so the
  /// buffer's spans were always the *greater* ones (#180 part B, item 6). A `peek` that reasons
  /// about the two — that sorts or merges by span, or that reads the buffer as a prefix of its
  /// own run and skips an entry it decides the caller already holds — was asked the same question
  /// every time, and answered it the same way a conforming cache would.
  ///
  /// The fixed value was also the **inverse of the real one**. `InputRef`'s peek fill pushes the
  /// parked token before it calls in, because the parked token is the front of the stream: it
  /// heads the window and the cache fills in behind it. So at the call site the buffer holds what
  /// comes *first*, which is the one arrangement in which a cache can talk itself into
  /// deduplicating against it.
  ///
  /// Reaching it costs no extra corpus. The residency is built one full peek window into the
  /// corpus instead of at its start, which leaves that window free to prefill from and needs
  /// exactly the `capacity + window` tokens [`run_pass`](Self::run_pass) already demands. One
  /// residency — the full cache — and the same depth sweep, since what is under test here is the
  /// span relation and not the depth.
  fn check_peek_behind_earlier_entries(&self, cap: usize) {
    if cap == 0 {
      return;
    }
    let window = <<PeekWindow as Window>::CAPACITY as Unsigned>::USIZE;
    let (cache, want) = self.filled_from(window, cap);
    let state = format!(
      "a full cache of {cap} built from the corpus tokens PAST the prefill, so every entry already in the buffer PRECEDES every resident one — the span relation the input layer's own peek fill produces, and the inverse of the one every other prefilled driver here builds"
    );
    for depth in 1..=window {
      self.check_prefilled_peek(&cache, cap, &want, depth, Prefill::Earlier, &state);
    }
  }

  /// [`filled`](Self::filled), but from corpus index `offset` rather than from the start — so
  /// that the tokens before it are available to prefill a peek buffer with spans that precede the
  /// whole residency.
  fn filled_from(&self, offset: usize, n: usize) -> (C, Vec<CachedTokenOf<'inp, L>>) {
    let name = self.label();
    let want_len = offset.saturating_add(n);
    let mut corpus = self.corpus(want_len);
    assert!(
      corpus.len() == want_len,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but the kit needs {want_len} to fill a cache of {n} starting {offset} token(s) into the corpus. This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );
    let mut cache = self.make();
    let mut want = Vec::with_capacity(n);
    for (i, tok) in corpus.split_off(offset).into_iter().enumerate() {
      let placed = self.accepted_push_back(
        &mut cache,
        tok,
        &format!("push_back #{i} filling from corpus offset {offset}"),
        format_args!(
          "tokora cache conformance [{name} bounded-peek]: push_back #{i} was REFUSED while filling a fresh cache to {n} of its capacity {n}; a push is refused only when the cache is FULL"
        ),
      );
      want.push(placed);
    }
    (cache, want)
  }

  // ── checks 6 and 8 where a ring has WRAPPED ─────────────────────────────────────

  /// Checks 6 and 8 against a residency whose live run runs **past the end of a ring's backing
  /// array and around to its start** — the one shape no driver in this kit could reach.
  ///
  /// [`filled`](Self::filled) pushes back into an empty cache, so a ring's head index starts at
  /// slot zero and every residency built from it satisfies `head + len <= capacity`. Popping the
  /// front advances the head and shortens the run by the same step, so that sum is invariant;
  /// popping the back only shortens it. **A run wraps only when something is pushed after
  /// something was popped**, and until this check existed nothing in the kit did that (#180 part
  /// B, item 5). So the classic missing `% capacity` — in the walk `peek` makes from the head,
  /// and in the end `span` computes from `head + len` — was truncating by zero everywhere it was
  /// asked.
  ///
  /// The rotation is swept, since `capacity - head` is where a truncated walk stops and a single
  /// head position fixes it. Two bounds cap the sweep. It stops one short of the capacity,
  /// because at `rotation == capacity` a ring's head is back at slot zero and the run does not
  /// wrap at all — that pass would certify nothing while looking like the deepest case. And it
  /// stops at the **peek window**, because the tokens pushed back come from past the residency
  /// and the corpus this kit asks its caller for is the capacity plus one window: a deeper
  /// rotation would be a longer source requirement charged to every user of the kit, for head
  /// positions that differ from these in no way a walk can tell.
  ///
  /// Only `peek` and `span` are re-driven here. They are the two operations that WALK from the
  /// head; `front`, `back`, `pop_front` and `pop_back` name a single slot, and an index that
  /// names the wrong slot is already caught wherever the kit checks residency —
  /// [`check_peek_at`](Self::check_peek_at)'s own [`assert_resident`](Self::assert_resident) does
  /// it here too.
  fn check_wrapped_ring(&self, cap: usize) {
    if cap < 2 {
      // At capacity 1 a ring's head is always slot zero, so there is no wrap to reach; at 0
      // there is no residency at all.
      return;
    }
    let window = <<PeekWindow as Window>::CAPACITY as Unsigned>::USIZE;
    for rotation in 1..=(cap - 1).min(window) {
      let (cache, want) = self.rotated(cap, rotation);
      let state = format!(
        "a WRAPPED residency: {cap} of {cap} resident after {rotation} pop_front(s) and {rotation} push_back(s), so a ring's live run starts at slot {rotation} and runs past the end of its array"
      );
      self.check_peek_at(&cache, cap, &want, &state);
      self.assert_span_at(&cache, &Self::spans_of(&want), &state);
    }
  }

  /// A fresh cache filled to capacity, drained `rotation` entries off the **front**, and topped
  /// back up to capacity with `rotation` fresh tokens — so a ring holding it has its head at slot
  /// `rotation` and a live run that wraps.
  ///
  /// The tokens pushed back are the ones past the original residency, not the ones just popped:
  /// re-pushing those would leave a resident sequence whose spans run backwards at the seam, and
  /// a combined span from a later start to an earlier end is not a value a `Span` implementation
  /// is obliged to be able to construct. A cache's residency is always in source order, and so is
  /// this one.
  fn rotated(&self, cap: usize, rotation: usize) -> (C, Vec<CachedTokenOf<'inp, L>>) {
    let name = self.label();
    let (mut cache, filled) = self.filled_to_capacity(cap);
    for (i, expected) in filled.iter().take(rotation).enumerate() {
      self.drained_front(
        &mut cache,
        expected,
        &format!("wrapped-run pop_front #{i}"),
        format_args!(
          "tokora cache conformance [{name} wrapped-run]: pop_front() answered None after {i} of {rotation} pops off a cache filled to its capacity {cap}"
        ),
        " These pops only exist to rotate the ring.",
      );
    }
    let mut want: Vec<CachedTokenOf<'inp, L>> = filled[rotation..].to_vec();
    for (i, tok) in self.beyond_residency(cap, rotation).into_iter().enumerate() {
      let placed = self.accepted_push_back(
        &mut cache,
        tok,
        &format!("push_back #{i} topping up a wrapped residency"),
        format_args!(
          "tokora cache conformance [{name} wrapped-run]: push_back #{i} was REFUSED while topping a cache back up to its capacity {cap} with {} of {cap} slots used; a push is refused only when the cache is FULL",
          want.len()
        ),
      );
      want.push(placed);
    }
    (cache, want)
  }

  // ── 9. pop_front_if / try_pop_front_if ──────────────────────────────────────────

  /// Both methods are DEFAULT `Cache` methods, composed of `front` + `pop_front` (already
  /// checked exhaustively elsewhere) — so an implementation that does not override them is
  /// correct for free. What this check exists for is an implementation that DOES override
  /// them, for whatever reason (a fused peek-and-pop, say): before it existed, neither method
  /// was ever called by the kit at all, so an override that removed on a false predicate, or
  /// removed and answered `None`, passed exactly like a conforming one (#180 part A, item 5).
  fn check_pop_front_if(&self, cap: usize) {
    let name = self.label();

    // Empty cache: the predicate must not run at all — there is no front to hand it — and both
    // methods answer `None` regardless of what the predicate would have said.
    //
    // Presence-only, and the law, for these two probes ONLY — not for the false-predicate probe
    // further down, which discarded its argument until #183 round 9 on the strength of this same
    // sentence. What makes these two sound is not that they answer `None`; it is that the
    // predicate must never run, which is asserted on its own line right after each call. On the
    // conforming path no entry is handed to anyone, so there is nothing to compare.
    // (Documented exception — see `checked_push`.)
    let mut empty = self.make();
    let ran = Cell::new(false);
    assert!(
      empty
        // CACHE_CALL_CENSUS: absence
        .pop_front_if(|_| {
          ran.set(true);
          true
        })
        .is_none(),
      "tokora cache conformance [{name} pop-front-if]: pop_front_if() answered Some on an empty cache"
    );
    assert!(
      !ran.get(),
      "tokora cache conformance [{name} pop-front-if]: pop_front_if()'s predicate ran against an empty cache, which has no front to hand it"
    );

    let mut empty2 = self.make();
    let ran2 = Cell::new(false);
    assert!(
      empty2
        // CACHE_CALL_CENSUS: absence
        .try_pop_front_if::<(), _>(|_| {
          ran2.set(true);
          Ok(())
        })
        .is_none(),
      "tokora cache conformance [{name} pop-front-if]: try_pop_front_if() answered Some on an empty cache"
    );
    assert!(
      !ran2.get(),
      "tokora cache conformance [{name} pop-front-if]: try_pop_front_if()'s predicate ran against an empty cache, which has no front to hand it"
    );

    if cap == 0 {
      return;
    }

    // A false predicate removes nothing and leaves every observable untouched — and is handed the
    // front entry on the way to being told so.
    //
    // The RETURN here is an absence law: `None` is the whole answer, and there is no entry behind
    // it. The predicate ARGUMENT is not, and classifying the whole site by its return is what left
    // this arm reading its argument not at all until #183's ninth round. `pop_front_if` hands
    // caller code a real entry *before* it learns the predicate's answer, so absence covers what
    // comes back and covers nothing about what went out: a cache can keep storage conforming,
    // answer `None` correctly, leave residency untouched, and still show the declining predicate
    // someone else's token or state — and every other oracle here would agree it conformed.
    //
    // The old justification for discarding it was that the true-predicate arm below already pins
    // this read, since `F: FnOnce` settles the entry before the `true`/`false` exists. That is an
    // argument about a CONFORMING cache. These are two separate monomorphisations of
    // `pop_front_if`, driven on two separately built instances, and an override is free to differ
    // between them — the same adversary `POP_FRONT_IF_PREDICATES_ON_THE_BACK` already assumes one
    // call shape away.
    let (mut cache_f, want_f) = self.filled(cap);
    let declined = self.recording_pop_front_if(
      &mut cache_f,
      &want_f[0],
      false,
      "pop-front-if pop_front_if()'s false-predicate argument",
      "pop_front_if()'s false predicate was handed",
    );
    // Presence-only, and the law, for the RETURN: a false predicate must remove nothing.
    assert!(
      declined.is_none(),
      "tokora cache conformance [{name} pop-front-if]: pop_front_if() removed an entry despite a false predicate"
    );
    self.assert_resident(
      &cache_f,
      cap,
      &want_f,
      "after pop_front_if() with a false predicate",
    );

    // An Err-returning predicate is refused the same way, and the error comes straight back — and
    // it too is handed the FRONT entry, which is recorded here rather than discarded.
    //
    // Both `try_pop_front_if` closures in this check used to be `|_| ...`, so everything the check
    // asserted about this method was its return value and its residency. An override that ran the
    // caller's validation predicate against `back()` and then produced the conforming return and
    // the conforming residency satisfied every one of them — a cache that decides whether to
    // remove or retain the front on the strength of unrelated lookahead, certified.
    let (mut cache_e, want_e) = self.filled(cap);
    // The predicate's closure lives in the router, so the entry it is handed is recorded and
    // compared unconditionally — it cannot be discarded by an edit here, which is what it was
    // before #183.
    let result = self.recording_try_pop_front_if(
      &mut cache_e,
      &want_e[0],
      Err("no"),
      "pop-front-if try_pop_front_if()'s Err-path predicate argument",
      "try_pop_front_if()'s Err-path predicate was handed",
    );
    assert!(
      matches!(result, Some(Err("no"))),
      "tokora cache conformance [{name} pop-front-if]: try_pop_front_if() with an Err-returning predicate did not hand the error straight back"
    );
    self.assert_resident(
      &cache_e,
      cap,
      &want_e,
      "after try_pop_front_if() with an Err predicate",
    );

    // A true predicate sees the FRONT entry, and removes exactly it.
    let (mut cache_t, want_t) = self.filled(cap);
    let popped = self.recording_pop_front_if(
      &mut cache_t,
      &want_t[0],
      true,
      "pop-front-if pop_front_if()'s predicate argument",
      "pop_front_if()'s predicate was handed",
    );
    self.assert_returned_front(
      popped.as_ref(),
      &want_t[0],
      "pop-front-if pop_front_if() with a true predicate",
      "pop_front_if() with a true predicate returned",
    );
    self.assert_resident(
      &cache_t,
      cap,
      &want_t[1..],
      "after pop_front_if() with a true predicate",
    );

    // try_pop_front_if with Ok(()) does the same, predicate argument included.
    //
    // Stated here as well as on the Err path above rather than left to whichever call runs first.
    // Against `TRY_POP_FRONT_IF_PREDICATES_ON_THE_BACK` it is the Err path — the earlier call —
    // that reports, and by the `FnOnce` argument above no cache can fail one of these and pass
    // the other; this assertion is what keeps the law stated where the Ok path is driven, so a
    // later driver that reorders the two, or drives one at a residency the other does not reach,
    // does not silently lose it.
    let (mut cache_ok, want_ok) = self.filled(cap);
    let popped_ok = self
      .recording_try_pop_front_if(
        &mut cache_ok,
        &want_ok[0],
        Ok(()),
        "pop-front-if try_pop_front_if()'s Ok-path predicate argument",
        "try_pop_front_if()'s Ok-path predicate was handed",
      )
      .and_then(Result::ok);
    self.assert_returned_front(
      popped_ok.as_ref(),
      &want_ok[0],
      "pop-front-if try_pop_front_if() with an Ok(()) predicate",
      "try_pop_front_if() with an Ok(()) predicate returned",
    );
    self.assert_resident(
      &cache_ok,
      cap,
      &want_ok[1..],
      "after try_pop_front_if() with an Ok(()) predicate",
    );
  }

  /// One recorded `pop_front_if`/`try_pop_front_if` predicate argument against the front entry.
  ///
  /// `lede` is the site's own wording, kept verbatim from before #183 because two cells pin it
  /// (`pop_front_if()'s predicate was handed`, `try_pop_front_if()'s Err-path predicate was
  /// handed`). Presence is checked here rather than at the call site so that "the predicate never
  /// ran" reports as itself instead of as a mismatch against `None`.
  fn assert_recorded_predicate_arg(
    &self,
    saw: Option<&PeekedTriple<'inp, L>>,
    want: &CachedTokenOf<'inp, L>,
    site: &str,
    lede: &str,
  ) {
    let name = self.label();
    let Some(saw) = saw else {
      panic!(
        "tokora cache conformance [{name} pop-front-if]: {lede} nothing — the predicate never ran, or recorded no entry, against a cache whose front is {:?}",
        span_of::<L>(want)
      )
    };
    self.assert_peeked_entry_eq(
      saw,
      want,
      site,
      format_args!(
        "tokora cache conformance [{name} pop-front-if]: {lede} {:?}, expected the front entry {:?}",
        saw.0,
        span_of::<L>(want)
      ),
    );
  }

  /// One `pop_front_if`/`try_pop_front_if` return value against the front entry, in full.
  ///
  /// `lede` is the site's own wording, kept verbatim for the same reason
  /// [`assert_recorded_predicate_arg`](Self::assert_recorded_predicate_arg)'s is.
  fn assert_returned_front(
    &self,
    got: Option<&CachedTokenOf<'inp, L>>,
    want: &CachedTokenOf<'inp, L>,
    site: &str,
    lede: &str,
  ) {
    let name = self.label();
    let Some(got) = got else {
      panic!(
        "tokora cache conformance [{name} pop-front-if]: {lede} None, expected the front entry {:?}",
        span_of::<L>(want)
      )
    };
    self.assert_owned_entry_eq(
      got,
      want,
      site,
      format_args!(
        "tokora cache conformance [{name} pop-front-if]: {lede} {:?}, expected the front entry {:?}",
        span_of::<L>(got),
        span_of::<L>(want)
      ),
      NO_NOTE,
    );
  }

  /// A borrowed entry — a `front`/`back` reference or a predicate argument — as the triple, so it
  /// can be recorded in a [`Cell`] and compared after the borrow it came from has ended.
  fn ref_triple(entry: &CachedTokenRefOf<'_, 'inp, L>) -> PeekedTriple<'inp, L> {
    (
      (**entry.token().span_ref()).clone(),
      (**entry.token().data()).clone(),
      (*entry.state()).clone(),
    )
  }

  // ── 10. push_many ────────────────────────────────────────────────────────────────

  /// `push_many` is also a DEFAULT method, composed of `push_back` (already checked
  /// exhaustively) — this exists for the same reason [`check_pop_front_if`](Self::check_pop_front_if)
  /// does: an override that silently discards what does not fit, rather than handing it back
  /// through the overflow iterator, passed unnoticed before this check existed at all (#180 part
  /// A, item 6).
  fn check_push_many(&self, cap: usize) {
    let name = self.label();
    let want_len = cap.saturating_add(2);
    let corpus = self.corpus(want_len);
    assert!(
      corpus.len() == want_len,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but push_many's check needs {want_len} — 2 more than the capacity {cap}, to exercise the overflow return. This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );
    // Cloned before `push_many` takes the originals by value: the overflow comparison below is
    // over whole entries since #183, so the expectation has to be the whole entry.
    let all: Vec<CachedTokenOf<'inp, L>> = corpus.clone();
    let all_spans = Self::spans_of(&all);

    let mut cache = self.make();
    // CACHE_CALL_CENSUS: compared-in-place
    let overflow: Vec<_> = cache.push_many(corpus.into_iter()).collect();
    let overflow_spans: Vec<L::Span> = overflow.iter().map(span_of::<L>).collect();
    assert!(
      overflow_spans == all_spans[cap..],
      "tokora cache conformance [{name} push-many]: push_many()'s overflow iterator yielded {overflow_spans:?}, expected exactly the {} refused entr(ies) {:?}, unchanged and in order",
      all_spans.len() - cap,
      &all_spans[cap..]
    );
    // "Unchanged" is the whole entry: a `push_many` that hands back the right spans in the right
    // order with re-associated tokens or states behind them has changed what it refused (#183).
    for (i, (got, expected)) in overflow.iter().zip(&all[cap..]).enumerate() {
      self.assert_owned_entry_eq(
        got,
        expected,
        &format!("push-many overflow entry #{i}"),
        format_args!(
          "tokora cache conformance kit bug [{name} push-many]: overflow entry #{i} has a span the vector comparison above should already have rejected"
        ),
        NO_NOTE,
      );
    }
    self.assert_resident(
      &cache,
      cap,
      &all[..cap],
      "after push_many() with 2 more tokens than capacity",
    );
  }
}
