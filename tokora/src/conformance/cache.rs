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
//!    an empty cache. (The input layer compiles its parked-slot fallback *out* on that
//!    declaration, so a cache that declares it and then refuses is losing tokens.)
//! 3. **FIFO append and exact length** — `push_back` places each token after every resident one;
//!    `len` is exactly the resident count and `remaining` exactly how many more `push_back`s will
//!    be accepted, at every step; and a refused push **returns the token unchanged** and leaves
//!    every observable untouched.
//! 4. **Order** — `pop_front` removes the oldest and `pop_back` the newest, and `front`/`back`
//!    view those same two entries without removing them. The input layer's restore path is built
//!    on the `pop_back` half.
//! 5. **`push_front` prepends** — before every resident entry, with the same refusal
//!    round-trip.
//! 6. **Bounded, pure `peek`** — appends exactly `min(len, buffer capacity)` entries, oldest
//!    first, each resident token once; changes no observable; and answers identically when
//!    called twice on an unchanged cache. `peek_one` agrees with `front`.
//! 7. **`clear`** — returns the cache to the check-1 state.
//! 8. **`span`** — the combined span runs from the front entry's start to the back entry's end.
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
//! **The non-panicking restore path** cannot be checked by running code: the input layer calls
//! `pop_front`, `pop_back`, `clear`, `front`, `front_span` and `len` from guard drops that may
//! run mid-unwind, where a panic aborts the process. A test cannot survive its own witness. The
//! law is on the trait; this kit exercises those six operations so a panicking one fails *here*,
//! in a test, rather than in a consumer's rollback.
//!
//! # Violation posture
//!
//! A failing check is a bug in the *cache* (or a mismatch with the documented contract),
//! surfaced loudly. The kit never mutates behaviour; it observes and asserts.

use core::marker::PhantomData;

use generic_arraydeque::{GenericArrayDeque, typenum::U4, typenum::Unsigned};

use crate::{
  Lexer, Span, Window,
  cache::{Cache, CachedToken, CachedTokenOf, MaybeRefCachedTokenOf, PeekedTokenExt},
  span::Spanned,
};

/// The peek window the kit peeks through. Four is enough to distinguish "bounded by the buffer"
/// from "bounded by the residency" on both sides of the default cache capacity.
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
/// # #[cfg(all(feature = "logos", feature = "std"))]
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
/// CacheHarness::<MyLexer<'_>, DefaultCache<'_, MyLexer<'_>>>::new("a b c d e").run();
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
  /// The source must yield at least two items, since a queue law about ordering is not
  /// observable on one element; `run` says so if it does not.
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
    let corpus = self.corpus(cap.saturating_add(2).max(4));
    let want = cap.saturating_add(1).max(2);
    assert!(
      corpus.len() >= want,
      "tokora cache conformance [{name}]: the source lexed {} token(s), but this cache's capacity is {cap} and the kit needs {want} — one past the capacity, so the refusal laws are observable at all (and at least 2, since an ordering law is not observable on one element). This is a kit-usage problem, not a cache defect: lengthen the source.",
      corpus.len()
    );

    self.check_empty(cap, "fresh");
    self.check_retains_front(cap);
    self.check_fifo_and_length(cap);
    self.check_pop_order(cap);
    self.check_push_front(cap);
    self.check_peek(cap);
    self.check_clear(cap);
    self.check_span(cap);
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
    let cache = C::new();
    self.assert_empty(&cache, cap, when);
  }

  fn assert_empty(&self, cache: &C, cap: usize, when: &str) {
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
    let mut probe = C::new();
    assert!(
      probe.pop_front().is_none() && probe.pop_back().is_none(),
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
      "tokora cache conformance [{name} retains-front]: push_front into an empty cache was refused while RETAINS_FRONT is declared true. The input layer compiles its parked-slot fallback OUT on that declaration, so a refusal here loses the token — declare `false` or retain the front."
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

  /// The three length/edge observables against the list the kit is tracking.
  fn assert_resident(&self, cache: &C, cap: usize, want: &[L::Span], when: &str) {
    let name = self.name;
    assert!(
      cache.len() == want.len(),
      "tokora cache conformance [{name} exact-length] {when}: len() is {}, expected {}",
      cache.len(),
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

    for tok in it {
      let span = span_of::<L>(&tok);
      let full = resident.len() == cap;
      match cache.push_front(tok) {
        Ok(_) => {
          assert!(
            !full,
            "tokora cache conformance [{name} exact-length]: push_front was accepted at capacity {cap}"
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
  }

  // ── 6. bounded, pure peek ───────────────────────────────────────────────────────

  fn check_peek(&self, cap: usize) {
    if cap == 0 {
      return;
    }
    let name = self.name;
    let (cache, want) = self.filled(cap);
    let window = <<PeekWindow as Window>::CAPACITY as Unsigned>::USIZE;
    let bound = window.min(want.len());

    let first = self.peeked_spans(&cache);
    assert!(
      first.len() == bound,
      "tokora cache conformance [{name} bounded-peek]: peek() appended {} entries with {} resident and a {window}-slot buffer, expected exactly min(len, capacity) = {bound}",
      first.len(),
      want.len()
    );
    assert!(
      first == want[..bound],
      "tokora cache conformance [{name} bounded-peek]: peek() appended {first:?}, expected the resident prefix OLDEST FIRST {:?}",
      &want[..bound]
    );

    // Purity: the cache is unchanged, and a second peek reads the same.
    self.assert_resident(&cache, cap, &want, "after peek()");
    let second = self.peeked_spans(&cache);
    assert!(
      second == first,
      "tokora cache conformance [{name} pure-peek]: two peeks on an unchanged cache disagreed: {first:?} then {second:?}. `peek` takes &self and must be logically pure."
    );

    // `peek_one` is the single-slot case, and it must name the front.
    let one = cache
      .peek_one()
      .unwrap_or_else(|| panic!("tokora cache conformance [{name} bounded-peek]: peek_one() is empty with {} entries resident", want.len()));
    assert!(
      *one.span() == want[0],
      "tokora cache conformance [{name} bounded-peek]: peek_one() named {:?}, expected the front entry {:?}",
      one.span(),
      want[0]
    );
  }

  /// The spans `peek` appends, in order.
  fn peeked_spans(&self, cache: &C) -> Vec<L::Span> {
    let mut buf: GenericArrayDeque<
      MaybeRefCachedTokenOf<'_, 'inp, L>,
      <PeekWindow as Window>::CAPACITY,
    > = GenericArrayDeque::new();
    cache.peek::<PeekWindow>(&mut buf);
    buf.iter().map(|entry| entry.span().clone()).collect()
  }

  // ── 7. clear ────────────────────────────────────────────────────────────────────

  fn check_clear(&self, cap: usize) {
    let (mut cache, _) = self.filled(cap);
    cache.clear();
    self.assert_empty(&cache, cap, "after clear()");
  }

  // ── 8. the combined span ────────────────────────────────────────────────────────

  fn check_span(&self, cap: usize) {
    if cap == 0 {
      return;
    }
    let name = self.name;
    let (cache, want) = self.filled(cap);
    let combined = cache.span().unwrap_or_else(|| {
      panic!(
        "tokora cache conformance [{name} combined-span]: span() is empty with {} entries resident",
        want.len()
      )
    });
    let expect_start = want[0].start_ref();
    let expect_end = want[want.len() - 1].end_ref();
    assert!(
      combined.start_ref() == expect_start && combined.end_ref() == expect_end,
      "tokora cache conformance [{name} combined-span]: span() is {combined:?}, expected {expect_start:?}..{expect_end:?} — the front entry's start to the back entry's end"
    );
  }
}
