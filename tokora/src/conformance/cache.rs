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
//!    on the `pop_back` half.
//! 5. **`push_front` prepends** — before every resident entry, with the same refusal
//!    round-trip, and the same requirement that a refusal be **warranted**: a push is refused
//!    only by a full cache, on this arm exactly as on `push_back`'s. The prepend order is
//!    checked in **full**, not at its two ends: the resident sequence is drained after each
//!    accepted prefix, since a `push_front` that lands its token at the front and permutes what
//!    is behind it has the right front, the right back and the right length. And the refusal law
//!    is driven at the **empty** cache too, at every nonzero capacity — the one state the
//!    prepend driver cannot reach, because it has to seed the cache with a `push_back` before
//!    there is anything to prepend before.
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
//!    that end reads them. `pop_back` is the input layer's restore path. `peek_one` agrees with
//!    `front`, and answers nothing where there is no front.
//! 7. **`clear`** — returns the cache to the check-1 state, AND the cache stays usable
//!    afterward: it is refilled with `cap` fresh pushes and checked against check 3's oracle,
//!    so a `clear` that empties the cache but also permanently disables it (poisons it, zeroes
//!    its capacity) does not pass by never being asked to accept another token.
//! 8. **`span`** — the combined span runs from the front entry's start to the back entry's end,
//!    swept at every residency check 6 sweeps (full, and partially drained from both ends), not
//!    checked once against a freshly filled cache: a `span` that is correct immediately after a
//!    fill and stale after any pop is indistinguishable from a conforming one at the one
//!    residency a single check reaches.
//! 9. **`pop_front_if` / `try_pop_front_if`** — the predicate sees the front entry and nothing
//!    else; a `false` (or `Err`) answer removes nothing and leaves every observable untouched; a
//!    `true` (or `Ok(())`) answer removes exactly the front entry, matching what a bare
//!    `pop_front` would return from an identically filled cache. Against an empty cache, both
//!    answer `None` and the predicate never runs at all — there is no front to hand it.
//! 10. **`push_many`** — accepts tokens in order until the cache is full, then hands the
//!     remainder back through the returned iterator, unchanged and in the order they arrived.
//!
//! The kit adapts to capacity: it runs every check a cache of that capacity can express, so the
//! capacity-0 `()` cache, the capacity-1 `Option` cache and an arbitrarily wide ring are all
//! driven by the same call.
//!
//! # What it deliberately does not check
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
//! **The non-panicking restore path** cannot be checked by running code: the input layer calls
//! `pop_front`, `pop_back`, `clear`, `front`, `front_span` and `len` from guard drops that may
//! run mid-unwind, where a panic aborts the process. A test cannot survive its own witness. The
//! law is on the trait; this kit exercises those six operations so a panicking one fails *here*,
//! in a test, rather than in a consumer's rollback.
//!
//! **Every oracle in this file compares spans, never tokens or state.** `span_of` takes a
//! `CachedToken` and returns only its `L::Span`, discarding the token and the `L::State` half —
//! and every check built on `front`, `back`, `pop_front`, `pop_back` or `peek` compares exactly
//! that: the span it gets back against the spans the corpus lexed, never the token or the state
//! sitting beside it in the same value. So a cache that returns the right spans while permuting
//! or corrupting the tokens or the states behind them passes every check in this kit, in full.
//!
//! That matters because `L::State` is not incidental payload: it is the half the input layer
//! restores the lexer *from*. `InputRef::lexer` rebuilds the lexer at the lookahead frontier from
//! exactly the state the newest retained token carries. A third-party cache with the right spans
//! and the wrong states does not merely fail to be certified by this kit — it passes, and then
//! corrupts every restore that resumes from the state it handed back.
//!
//! It is not closed here. Closing it needs a **heterogeneous** corpus — the in-tree corpus
//! (`cache_tests.rs`'s `CTok`) is a single token kind, so even a token-identity comparison could
//! not discriminate a permuted or substituted token today — **and** every oracle in this file
//! changed to compare more than spans. That is a change to the kit's fundamental observable,
//! deliberately deferred rather than rushed before a release.
//!
//! # Violation posture
//!
//! A failing check is a bug in the *cache* (or a mismatch with the documented contract),
//! surfaced loudly. The kit never mutates behaviour; it observes and asserts.

use std::{format, vec::Vec};

use core::{cell::Cell, marker::PhantomData};

use generic_arraydeque::{GenericArrayDeque, typenum::U4, typenum::Unsigned};
use mayber::Maybe;

use crate::{
  Lexer, Span, Window,
  cache::{Cache, CachedToken, CachedTokenOf, MaybeRefCachedTokenOf, PeekedTokenExt},
  span::Spanned,
};

/// The peek window the kit peeks through. Four is enough to distinguish "bounded by the buffer"
/// from "bounded by the residency" on both sides of the default cache capacity, and to give the
/// prefill sweep four distinct shapes to hand `peek`: a prefix of one, two intermediate depths
/// with an order of their own and room still behind them, and a buffer with no room left. It is
/// also what every source the kit is pointed at has to carry past the cache's capacity, so it
/// buys those shapes at four tokens rather than at a longer corpus.
type PeekWindow = U4;

/// The span of a cached token, owned. `token()` hands back a `Spanned<&T, &Span>`, so the span
/// arrives double-borrowed and has to be dereferenced once before it can be cloned.
fn span_of<'inp, L>(tok: &CachedTokenOf<'inp, L>) -> L::Span
where
  L: Lexer<'inp>,
{
  (*tok.token().span_ref()).clone()
}

/// A conformance harness that drives a [`Cache`] implementation `C` against the cache contract.
///
/// The corpus is lexed from a source with `L`, so the kit needs no way to fabricate tokens and
/// the tokens it pushes are real ones. Build one, then call [`run`](Self::run).
///
/// # Example
///
/// ```
/// # #[cfg(all(any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"), feature = "std"))]
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
  _cache: PhantomData<fn() -> C>,
  _lang: PhantomData<fn(&Lang)>,
}

impl<'inp, L, C, Lang> CacheHarness<'inp, L, C, Lang>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Lang: ?Sized,
  C: Cache<'inp, L, Lang>,
{
  /// Creates a harness over `source`, from which the kit lexes its token corpus.
  ///
  /// The source must yield the cache's capacity plus a full peek window of items — the residency
  /// the checks build, plus the tokens past it that check 6 prefills the peek buffer with — and
  /// at least two in any case, since a queue law about ordering is not observable on one element.
  /// `run` says so, with the number, if it does not.
  #[must_use]
  pub fn new(source: &'inp L::Source) -> Self {
    Self {
      source,
      name: "cache",
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

  /// Runs every check the cache's capacity can express, panicking on the first violation.
  ///
  /// # Panics
  ///
  /// Panics — naming the check, the capacity, and the expected-vs-got values — the moment a
  /// contract law fails. Returns normally on full conformance.
  pub fn run(&self) {
    let name = self.name;
    let cap = C::new().remaining();
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
    self.check_clear(cap);
    self.check_span(cap);
    self.check_pop_front_if(cap);
    self.check_push_many(cap);
  }

  // ── the corpus ──────────────────────────────────────────────────────────────────

  /// Lexes up to `want` successful items out of the source, each paired with the state that
  /// produced it — exactly the shape the input layer caches.
  fn corpus(&self, want: usize) -> Vec<CachedTokenOf<'inp, L>> {
    let mut lexer = L::new(self.source);
    let mut out = Vec::new();
    while out.len() < want {
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
  /// Returns the cache and the spans it should be holding, so every later assertion compares
  /// against a list the kit built rather than against the cache's own answer.
  fn filled(&self, n: usize) -> (C, Vec<L::Span>) {
    let mut cache = C::new();
    let mut want = Vec::new();
    for tok in self.corpus(n) {
      let span = span_of::<L>(&tok);
      if cache.push_back(tok).is_ok() {
        want.push(span);
      }
    }
    (cache, want)
  }

  // ── 1. empty invariants ─────────────────────────────────────────────────────────

  fn check_empty(&self, cap: usize, when: &str) {
    let mut cache = C::new();
    self.assert_empty(&mut cache, cap, when);
  }

  /// Takes `cache` by `&mut` specifically so its own pop methods can be probed below — see
  /// there for why a fresh `C::new()` used to stand in for it and what that missed.
  fn assert_empty(&self, cache: &mut C, cap: usize, when: &str) {
    let name = self.name;
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
    assert!(
      cache.front().is_none() && cache.back().is_none(),
      "tokora cache conformance [{name} empty-invariants/{when}]: front()/back() answered on an empty cache"
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
    assert!(
      cache.pop_front().is_none() && cache.pop_back().is_none(),
      "tokora cache conformance [{name} empty-invariants/{when}]: a pop answered on an empty cache"
    );
    assert!(
      cache.peek_one().is_none(),
      "tokora cache conformance [{name} empty-invariants/{when}]: peek_one() answered on an empty cache"
    );
    assert!(
      self.peeked_spans(cache).is_empty(),
      "tokora cache conformance [{name} empty-invariants/{when}]: peek() appended entries from an empty cache"
    );
  }

  // ── 2. RETAINS_FRONT honesty ────────────────────────────────────────────────────

  fn check_retains_front(&self, cap: usize) {
    if !C::RETAINS_FRONT {
      return;
    }
    let name = self.name;
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
    let mut fresh = C::new();
    let first = self
      .corpus(1)
      .pop()
      .expect("run() checked the corpus is non-empty");
    let first_span = span_of::<L>(&first);
    assert!(
      fresh.push_front(first).is_ok(),
      "tokora cache conformance [{name} retains-front]: the FIRST operation on a fresh cache — a push_front while it is empty — was refused while RETAINS_FRONT is declared true. The input layer compiles its parked-slot fallback OUT on that declaration, so a refusal here loses the token — declare `false` or retain the front."
    );
    self.assert_resident(
      &fresh,
      cap,
      core::slice::from_ref(&first_span),
      "after the first-ever operation, a push_front into a fresh cache",
    );

    // And again on an empty cache that has been used: a cache that retains the front until it is
    // drained once, and then stops, is the same violation reached from the other side.
    let mut cache = C::new();
    let tok = self.corpus(1).pop().expect("the corpus is non-empty");
    assert!(
      cache.push_back(tok).is_ok(),
      "tokora cache conformance [{name} retains-front]: push_back into an empty cache was refused while RETAINS_FRONT is true"
    );
    let popped = cache
      .pop_front()
      .expect("just pushed one entry, so a pop must answer");
    assert!(
      cache.push_front(popped).is_ok(),
      "tokora cache conformance [{name} retains-front]: push_front into an emptied cache was refused while RETAINS_FRONT is declared true. The input layer compiles its parked-slot fallback OUT on that declaration, so a refusal here loses the token — declare `false` or retain the front."
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
    let name = self.name;
    let declares = C::RETAINS_FRONT;

    // Fresh: the front push is the first operation this cache has ever seen.
    let mut fresh = C::new();
    let tok = self
      .corpus(1)
      .pop()
      .expect("run() checked the corpus is non-empty");
    let span = span_of::<L>(&tok);
    assert!(
      fresh.push_front(tok).is_ok(),
      "tokora cache conformance [{name} push-front/into-empty]: a push_front into an EMPTY cache — the first operation this one has seen, with all {cap} slots free — was REFUSED. A push is refused only when the cache is FULL, on this arm exactly as on push_back's, and an empty cache with capacity is not full. RETAINS_FRONT is declared {declares}, which decides what the refusal costs the input layer (a parked slot, or a lost token where the fallback is compiled out), not whether it is allowed."
    );
    self.assert_resident(
      &fresh,
      cap,
      core::slice::from_ref(&span),
      "after a push_front into a fresh, empty cache",
    );

    // Used and emptied: the same state reached the other way, for a cache whose front is
    // established lazily and then torn down again with the entry that established it.
    let mut used = C::new();
    let tok = self.corpus(1).pop().expect("the corpus is non-empty");
    assert!(
      used.push_back(tok).is_ok(),
      "tokora cache conformance [{name} push-front/into-empty]: push_back into an empty cache with {cap} slots free was refused"
    );
    let popped = used
      .pop_front()
      .expect("just pushed one entry, so a pop must answer");
    let span = span_of::<L>(&popped);
    assert!(
      used.push_front(popped).is_ok(),
      "tokora cache conformance [{name} push-front/into-empty]: a push_front into a cache that was filled and then emptied — all {cap} slots free again — was REFUSED. A push is refused only when the cache is FULL, and this one holds nothing."
    );
    self.assert_resident(
      &used,
      cap,
      core::slice::from_ref(&span),
      "after a push_front into a used, emptied cache",
    );
  }

  // ── 3. FIFO append, exact length, and the refusal round-trip ────────────────────

  fn check_fifo_and_length(&self, cap: usize) {
    let name = self.name;
    let mut cache = C::new();
    let corpus = self.corpus(cap.saturating_add(1).max(2));
    let mut resident: Vec<L::Span> = Vec::new();

    for (i, tok) in corpus.into_iter().enumerate() {
      let span = span_of::<L>(&tok);
      let expect_accept = resident.len() < cap;
      match cache.push_back(tok) {
        Ok(_) => {
          assert!(
            expect_accept,
            "tokora cache conformance [{name} exact-length]: push_back #{i} was ACCEPTED with {} entries resident and a capacity of {cap}; `remaining` had already reached 0, so this push contradicts it",
            resident.len()
          );
          resident.push(span);
        }
        Err(returned) => {
          assert!(
            !expect_accept,
            "tokora cache conformance [{name} exact-length]: push_back #{i} was REFUSED with {} of {cap} slots used; `remaining` promised it would be accepted",
            resident.len()
          );
          // The refusal round-trip: the token comes back unchanged, and nothing moved.
          let back_span = span_of::<L>(&returned);
          assert!(
            back_span == span,
            "tokora cache conformance [{name} refusal-round-trip]: a refused push_back returned a DIFFERENT token: pushed span {span:?}, got back {back_span:?}. A refusal must hand the caller its token unchanged."
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
  fn assert_resident(&self, cache: &C, cap: usize, want: &[L::Span], when: &str) {
    let name = self.name;
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
    match (want.first(), cache.front_span()) {
      (Some(expected), Some(got)) => assert!(
        got == expected,
        "tokora cache conformance [{name} fifo-append] {when}: front is {got:?}, expected the OLDEST resident entry {expected:?}"
      ),
      (None, None) => {}
      (a, b) => panic!(
        "tokora cache conformance [{name} fifo-append] {when}: front presence disagrees — expected {a:?}, got {b:?}"
      ),
    }
    match (want.last(), cache.back_span()) {
      (Some(expected), Some(got)) => assert!(
        got == expected,
        "tokora cache conformance [{name} fifo-append] {when}: back is {got:?}, expected the NEWEST resident entry {expected:?}. `push_back` appends after every resident entry."
      ),
      (None, None) => {}
      (a, b) => panic!(
        "tokora cache conformance [{name} fifo-append] {when}: back presence disagrees — expected {a:?}, got {b:?}"
      ),
    }
  }

  // ── 4. pop order ────────────────────────────────────────────────────────────────

  fn check_pop_order(&self, cap: usize) {
    if cap == 0 {
      return;
    }
    let name = self.name;

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
      let popped = cache
        .pop_front()
        .unwrap_or_else(|| panic!("tokora cache conformance [{name} order]: pop_front() is empty with {} entries still to pop", want.len() - i));
      let got = span_of::<L>(&popped);
      assert!(
        got == *expected,
        "tokora cache conformance [{name} order]: pop_front #{i} returned {got:?}, expected the OLDEST resident entry {expected:?}"
      );
      assert!(
        viewed == got,
        "tokora cache conformance [{name} order]: front() viewed {viewed:?} but pop_front() removed {got:?}; they must name the same entry"
      );
    }
    assert!(
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
      let popped = cache.pop_back().unwrap_or_else(|| {
        panic!("tokora cache conformance [{name} order]: pop_back() is empty mid-drain")
      });
      let got = span_of::<L>(&popped);
      assert!(
        got == *expected,
        "tokora cache conformance [{name} order]: pop_back #{i} returned {got:?}, expected the NEWEST resident entry {expected:?}. The input layer drops an abandoned continuation's entries with a run of pop_back calls, so a pop_back that is not newest-first drops the wrong tokens."
      );
      assert!(
        viewed == got,
        "tokora cache conformance [{name} order]: back() viewed {viewed:?} but pop_back() removed {got:?}; they must name the same entry"
      );
    }
    assert!(
      cache.pop_back().is_none(),
      "tokora cache conformance [{name} order]: pop_back() still answers after every resident entry was drained"
    );
  }

  // ── 5. push_front prepends ──────────────────────────────────────────────────────

  fn check_push_front(&self, cap: usize) {
    if cap < 2 {
      return;
    }
    let name = self.name;
    let corpus = self.corpus(cap.saturating_add(1));
    let mut cache = C::new();
    let mut resident: Vec<L::Span> = Vec::new();

    // One at the back, then everything else at the front: each must land BEFORE the rest.
    let mut it = corpus.into_iter();
    let first = it.next().expect("the corpus has at least two tokens");
    let first_span = span_of::<L>(&first);
    assert!(
      cache.push_back(first).is_ok(),
      "tokora cache conformance [{name} push-front]: the first push_back into an empty cache was refused"
    );
    resident.push(first_span);

    for (i, tok) in it.enumerate() {
      let span = span_of::<L>(&tok);
      let full = resident.len() == cap;
      match cache.push_front(tok) {
        Ok(_) => {
          assert!(
            !full,
            "tokora cache conformance [{name} exact-length]: push_front #{i} was accepted at capacity {cap}"
          );
          resident.insert(0, span.clone());
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
          // Without this the two halves of the same law were asymmetric, and the round-trip and
          // residency checks below — which a cache that simply declines everything satisfies
          // trivially — were the whole of what a refusal had to survive.
          assert!(
            full,
            "tokora cache conformance [{name} exact-length]: push_front #{i} was REFUSED with {} of {cap} slots used; a push is refused only when the cache is FULL. The input layer's put-back lands here: a refusal parks the token in the fallback slot (or panics outright, where RETAINS_FRONT is declared), so refusing early spends a resource the cache had no need of.",
            resident.len()
          );
          let back_span = span_of::<L>(&returned);
          assert!(
            back_span == span,
            "tokora cache conformance [{name} refusal-round-trip]: a refused push_front returned a DIFFERENT token: pushed {span:?}, got back {back_span:?}"
          );
          self.assert_resident(&cache, cap, &resident, "after a refused push_front");
          continue;
        }
      }
      self.assert_resident(&cache, cap, &resident, "after push_front");
    }
    assert!(
      resident.len() > 1,
      "tokora cache conformance [{name} push-front]: no push_front was ever accepted at capacity {cap}, so the prepend law was never observed"
    );

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
        let popped = mixed.pop_front().unwrap_or_else(|| {
          panic!(
            "tokora cache conformance [{name} push-front/full-order at depth {depth}]: pop_front() is empty at position {i} of the {} entries one push_back and {depth} push_front(s) put in",
            order.len()
          )
        });
        let got = span_of::<L>(&popped);
        assert!(
          got == *expected,
          "tokora cache conformance [{name} push-front/full-order at depth {depth}]: position {i} of the drained sequence is {got:?}, expected {expected:?}. After one push_back and {depth} push_front(s) the resident order is {order:?} — the front pushes newest-first, then the token that went to the back. `push_front` places its token before every resident entry and MOVES NONE OF THEM; front, back and len do not say that between them, since a push_front that permutes the interior leaves all three right."
        );
      }
      assert!(
        mixed.pop_front().is_none(),
        "tokora cache conformance [{name} push-front/full-order at depth {depth}]: pop_front() still answers after all {} entries were drained",
        order.len()
      );
    }
  }

  /// A fresh cache in the shape [`check_push_front`](Self::check_push_front) builds — one
  /// `push_back`, then `depth` `push_front`s — together with the resident order it must be
  /// holding: the front-pushed tokens newest-first, then the one that went to the back.
  ///
  /// `depth` is bounded by `cap - 1`, so every push here has room and a refusal is a violation
  /// rather than a case to handle — the warranted-refusal law itself is checked by the caller,
  /// which drives one push past the capacity.
  fn front_built(&self, cap: usize, depth: usize) -> (C, Vec<L::Span>) {
    let name = self.name;
    let want = depth.saturating_add(1);
    let corpus = self.corpus(want);
    assert!(
      corpus.len() == want,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but the kit needs {want} to build a cache of one push_back and {depth} push_front(s) at capacity {cap}. This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );

    let mut cache = C::new();
    let mut order: Vec<L::Span> = Vec::new();
    let mut it = corpus.into_iter();
    let back = it.next().expect("the corpus was just checked non-empty");
    let back_span = span_of::<L>(&back);
    assert!(
      cache.push_back(back).is_ok(),
      "tokora cache conformance [{name} push-front/full-order at depth {depth}]: the seeding push_back into an empty cache was refused at capacity {cap}"
    );
    order.push(back_span);
    for (i, tok) in it.enumerate() {
      let span = span_of::<L>(&tok);
      assert!(
        cache.push_front(tok).is_ok(),
        "tokora cache conformance [{name} push-front/full-order at depth {depth}]: push_front #{i} was REFUSED with {} of {cap} slots used; a push is refused only when the cache is FULL",
        order.len()
      );
      order.insert(0, span);
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
    let name = self.name;

    // Drained from the FRONT: the consuming path, leaving the resident suffix.
    for popped in 0..=cap {
      let (mut cache, filled) = self.filled_to_capacity(cap);
      for i in 0..popped {
        assert!(
          cache.pop_front().is_some(),
          "tokora cache conformance [{name} bounded-peek]: pop_front() answered None after {i} of {popped} pops off a cache filled to its capacity {cap}"
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
        assert!(
          cache.pop_back().is_some(),
          "tokora cache conformance [{name} bounded-peek]: pop_back() answered None after {i} of {popped} pops off a cache filled to its capacity {cap}"
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

  /// [`filled`](Self::filled) at exactly the capacity, which is what both residency sweeps in
  /// [`check_peek`](Self::check_peek) read their expectation off: each slices that list, so it has
  /// to be the full residency before the first pop.
  ///
  /// Check 3 drives the same fill one token further and would have failed already; this says so
  /// in the kit's own words rather than as a slice index panic.
  fn filled_to_capacity(&self, cap: usize) -> (C, Vec<L::Span>) {
    let name = self.name;
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
  fn check_peek_at(&self, cache: &C, cap: usize, want: &[L::Span], state: &str) {
    let name = self.name;
    let window = <<PeekWindow as Window>::CAPACITY as Unsigned>::USIZE;
    let bound = window.min(want.len());

    let first = self.peeked_spans(cache);
    assert!(
      first.len() == bound,
      "tokora cache conformance [{name} bounded-peek] against {state}: peek() appended {} entries into an empty {window}-slot buffer, expected exactly min(len, the buffer's remaining capacity) = {bound}. The bound is the cache's CURRENT length, not the room its backing store has: an entry that has been popped is not lookahead any more.",
      first.len()
    );
    assert!(
      first == want[..bound],
      "tokora cache conformance [{name} bounded-peek] against {state}: peek() appended {first:?}, expected the resident prefix OLDEST FIRST {:?}",
      &want[..bound]
    );

    // Purity: the cache is unchanged, and a second peek reads the same.
    self.assert_resident(cache, cap, want, &format!("after peek() against {state}"));
    let second = self.peeked_spans(cache);
    assert!(
      second == first,
      "tokora cache conformance [{name} pure-peek] against {state}: two peeks on an unchanged cache disagreed: {first:?} then {second:?}. `peek` takes &self and must be logically pure."
    );

    // `peek_one` is the single-slot case: it names the front, and it names nothing where a
    // drained cache has no front left to name.
    match cache.peek_one() {
      Some(one) => {
        let Some(expected) = want.first() else {
          panic!(
            "tokora cache conformance [{name} bounded-peek] against {state}: peek_one() answered {:?} with NOTHING resident",
            one.span()
          )
        };
        assert!(
          one.span() == expected,
          "tokora cache conformance [{name} bounded-peek] against {state}: peek_one() named {:?}, expected the front entry {expected:?}",
          one.span()
        );
      }
      None => assert!(
        want.is_empty(),
        "tokora cache conformance [{name} bounded-peek] against {state}: peek_one() is empty with {} entries resident",
        want.len()
      ),
    }

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
      self.check_prefilled_peek(cache, cap, want, depth, state);
    }
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
    want: &[L::Span],
    depth: usize,
    state: &str,
  ) {
    let name = self.name;
    let window = <<PeekWindow as Window>::CAPACITY as Unsigned>::USIZE;
    let prefill = self.beyond_residency(cap, depth);
    let prefill_spans: Vec<L::Span> = prefill.iter().map(|tok| span_of::<L>(tok)).collect();
    let remaining_at_prefill = window - depth;
    let prefilled_bound = remaining_at_prefill.min(want.len());
    let (prefix_after, appended) = self.peeked_spans_after_prefill(cache, prefill);
    assert!(
      prefix_after == prefill_spans,
      "tokora cache conformance [{name} bounded-peek/prefilled at depth {depth}] against {state}: peek() changed the {depth} entr(ies) already in the buffer ahead of it — got {prefix_after:?}, expected them untouched, {prefill_spans:?}. `peek` appends BEHIND what the buffer already holds; it neither overwrites nor reorders it."
    );
    assert!(
      appended.len() == prefilled_bound,
      "tokora cache conformance [{name} bounded-peek/prefilled at depth {depth}] against {state}: peek() appended {} entries into a buffer already holding {depth} of {window} slots, expected exactly min(len, buffer's REMAINING capacity) = {prefilled_bound}.",
      appended.len()
    );
    assert!(
      appended == want[..prefilled_bound],
      "tokora cache conformance [{name} bounded-peek/prefilled at depth {depth}] against {state}: peek() appended {appended:?}, expected the resident prefix OLDEST FIRST {:?}",
      &want[..prefilled_bound]
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
      self.peeked_spans_after_prefill(cache, self.beyond_residency(cap, depth));
    assert!(
      prefix_again == prefix_after && appended_again == appended,
      "tokora cache conformance [{name} pure-peek/prefilled at depth {depth}] against {state}: two prefilled peeks on an unchanged cache disagreed: prefix {prefix_after:?} then {prefix_again:?}, appended {appended:?} then {appended_again:?}. `peek` takes &self and must be logically pure against every shape of buffer, not only against an empty one."
    );
    self.assert_resident(
      cache,
      cap,
      want,
      &format!("after a second prefilled peek() at depth {depth} against {state}"),
    );
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
    let name = self.name;
    let want = cap.saturating_add(depth);
    let mut corpus = self.corpus(want);
    assert!(
      corpus.len() == want,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but the kit needs {want} — {depth} past the capacity {cap} — for a peek-buffer prefill of {depth} entr(ies) the cache is not itself holding. This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );
    corpus.split_off(cap)
  }

  /// The spans `peek` appends, in order, into a fresh, empty buffer.
  fn peeked_spans(&self, cache: &C) -> Vec<L::Span> {
    let mut buf: GenericArrayDeque<
      MaybeRefCachedTokenOf<'_, 'inp, L>,
      <PeekWindow as Window>::CAPACITY,
    > = GenericArrayDeque::new();
    cache.peek::<PeekWindow>(&mut buf);
    buf.iter().map(|entry| entry.span().clone()).collect()
  }

  /// The prefilled-buffer counterpart of [`peeked_spans`](Self::peeked_spans): loads `prefill`
  /// into the buffer first, calls `peek`, then splits the result into the spans standing where
  /// the original prefix was (to check `peek` left it alone) and the spans `peek` appended
  /// behind it, in order. This is the other shape the real call site hands `Cache::peek` —
  /// already holding a parked token or staged overflow, where [`peeked_spans`](Self::peeked_spans)
  /// is the empty-`buf` shape a cache hit or an overflow-free fill hands over — see
  /// [`check_peek`](Self::check_peek) for what that shape catches, and for the one thing it
  /// provably cannot.
  fn peeked_spans_after_prefill(
    &self,
    cache: &C,
    prefill: Vec<CachedTokenOf<'inp, L>>,
  ) -> (Vec<L::Span>, Vec<L::Span>) {
    let prefill_len = prefill.len();
    let mut buf: GenericArrayDeque<
      MaybeRefCachedTokenOf<'_, 'inp, L>,
      <PeekWindow as Window>::CAPACITY,
    > = GenericArrayDeque::new();
    for tok in prefill {
      assert!(
        buf.push_back(Maybe::Owned(tok)).is_none(),
        "tokora cache conformance kit bug: its own prefill overflowed the peek window before `peek` was even called"
      );
    }
    cache.peek::<PeekWindow>(&mut buf);
    let mut spans = buf.iter().map(|entry| entry.span().clone());
    let prefix = spans.by_ref().take(prefill_len).collect();
    let appended = spans.collect();
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
    let name = self.name;
    let mut want = Vec::with_capacity(cap);
    for tok in self.corpus(cap) {
      let span = span_of::<L>(&tok);
      assert!(
        cache.push_back(tok).is_ok(),
        "tokora cache conformance [{name} clear]: push_back() was refused while refilling a cache clear() just emptied — clear() must not leave the cache permanently unable to accept pushes"
      );
      want.push(span);
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
    let name = self.name;

    // Drained from the FRONT: the consuming path, leaving the resident suffix.
    for popped in 0..=cap {
      let (mut cache, filled) = self.filled_to_capacity(cap);
      for i in 0..popped {
        assert!(
          cache.pop_front().is_some(),
          "tokora cache conformance [{name} combined-span]: pop_front() answered None after {i} of {popped} pops off a cache filled to its capacity {cap}"
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
      self.assert_span_at(&cache, want, &state);
    }

    // Drained from the BACK: the restore path, leaving the resident prefix.
    for popped in 1..=cap {
      let (mut cache, filled) = self.filled_to_capacity(cap);
      for i in 0..popped {
        assert!(
          cache.pop_back().is_some(),
          "tokora cache conformance [{name} combined-span]: pop_back() answered None after {i} of {popped} pops off a cache filled to its capacity {cap}"
        );
      }
      let want = &filled[..cap - popped];
      let state = format!(
        "a partly drained cache: {} of {cap} resident, after {popped} pop_back(s) off a full one — the input layer's restore path",
        want.len()
      );
      self.assert_span_at(&cache, want, &state);
    }
  }

  /// Check 8 against a cache holding exactly `want`: `span()` runs from the front entry's start
  /// to the back entry's end, and is absent exactly when nothing is resident (the latter is
  /// also [`assert_empty`](Self::assert_empty)'s concern; asserted here too so every residency
  /// [`check_span`](Self::check_span) visits — including the fully drained one — is covered by
  /// the same oracle instead of splitting the empty case out).
  fn assert_span_at(&self, cache: &C, want: &[L::Span], state: &str) {
    let name = self.name;
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

  // ── 9. pop_front_if / try_pop_front_if ──────────────────────────────────────────

  /// Both methods are DEFAULT `Cache` methods, composed of `front` + `pop_front` (already
  /// checked exhaustively elsewhere) — so an implementation that does not override them is
  /// correct for free. What this check exists for is an implementation that DOES override
  /// them, for whatever reason (a fused peek-and-pop, say): before it existed, neither method
  /// was ever called by the kit at all, so an override that removed on a false predicate, or
  /// removed and answered `None`, passed exactly like a conforming one (#180 part A, item 5).
  fn check_pop_front_if(&self, cap: usize) {
    let name = self.name;

    // Empty cache: the predicate must not run at all — there is no front to hand it — and both
    // methods answer `None` regardless of what the predicate would have said.
    let mut empty = C::new();
    let ran = Cell::new(false);
    assert!(
      empty
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

    let mut empty2 = C::new();
    let ran2 = Cell::new(false);
    assert!(
      empty2
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

    // A false predicate removes nothing and leaves every observable untouched.
    let (mut cache_f, want_f) = self.filled(cap);
    assert!(
      cache_f.pop_front_if(|_| false).is_none(),
      "tokora cache conformance [{name} pop-front-if]: pop_front_if() removed an entry despite a false predicate"
    );
    self.assert_resident(
      &cache_f,
      cap,
      &want_f,
      "after pop_front_if() with a false predicate",
    );

    // An Err-returning predicate is refused the same way, and the error comes straight back.
    let (mut cache_e, want_e) = self.filled(cap);
    let result = cache_e.try_pop_front_if::<&str, _>(|_| Err("no"));
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

    // A true predicate sees the FRONT entry's span, and removes exactly it.
    let (mut cache_t, want_t) = self.filled(cap);
    let seen = Cell::new(None);
    let popped_span = cache_t
      .pop_front_if(|tok| {
        // `tok`'s own generic params are already references (`CachedTokenRefOf`), so `.token()`
        // and `.span_ref()` each add one more — double what `span_of` derefs for an OWNED
        // `CachedTokenOf`, hence the extra `*` here.
        seen.set(Some((**tok.token().span_ref()).clone()));
        true
      })
      .map(|tok| span_of::<L>(&tok));
    assert!(
      seen.take() == Some(want_t[0].clone()),
      "tokora cache conformance [{name} pop-front-if]: pop_front_if()'s predicate did not see the front entry {:?}",
      want_t[0]
    );
    assert!(
      popped_span == Some(want_t[0].clone()),
      "tokora cache conformance [{name} pop-front-if]: pop_front_if() with a true predicate returned {popped_span:?}, expected the front entry {:?}",
      want_t[0]
    );
    self.assert_resident(
      &cache_t,
      cap,
      &want_t[1..],
      "after pop_front_if() with a true predicate",
    );

    // try_pop_front_if with Ok(()) does the same.
    let (mut cache_ok, want_ok) = self.filled(cap);
    let popped_ok = cache_ok
      .try_pop_front_if::<&str, _>(|_| Ok(()))
      .and_then(Result::ok)
      .map(|tok| span_of::<L>(&tok));
    assert!(
      popped_ok == Some(want_ok[0].clone()),
      "tokora cache conformance [{name} pop-front-if]: try_pop_front_if() with an Ok(()) predicate returned {popped_ok:?}, expected the front entry {:?}",
      want_ok[0]
    );
    self.assert_resident(
      &cache_ok,
      cap,
      &want_ok[1..],
      "after try_pop_front_if() with an Ok(()) predicate",
    );
  }

  // ── 10. push_many ────────────────────────────────────────────────────────────────

  /// `push_many` is also a DEFAULT method, composed of `push_back` (already checked
  /// exhaustively) — this exists for the same reason [`check_pop_front_if`](Self::check_pop_front_if)
  /// does: an override that silently discards what does not fit, rather than handing it back
  /// through the overflow iterator, passed unnoticed before this check existed at all (#180 part
  /// A, item 6).
  fn check_push_many(&self, cap: usize) {
    let name = self.name;
    let want_len = cap.saturating_add(2);
    let corpus = self.corpus(want_len);
    assert!(
      corpus.len() == want_len,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but push_many's check needs {want_len} — 2 more than the capacity {cap}, to exercise the overflow return. This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );
    let all_spans: Vec<L::Span> = corpus.iter().map(span_of::<L>).collect();

    let mut cache = C::new();
    let overflow: Vec<_> = cache.push_many(corpus.into_iter()).collect();
    let overflow_spans: Vec<L::Span> = overflow.iter().map(span_of::<L>).collect();
    assert!(
      overflow_spans == all_spans[cap..],
      "tokora cache conformance [{name} push-many]: push_many()'s overflow iterator yielded {overflow_spans:?}, expected exactly the {} refused entr(ies) {:?}, unchanged and in order",
      all_spans.len() - cap,
      &all_spans[cap..]
    );
    self.assert_resident(
      &cache,
      cap,
      &all_spans[..cap],
      "after push_many() with 2 more tokens than capacity",
    );
  }
}
