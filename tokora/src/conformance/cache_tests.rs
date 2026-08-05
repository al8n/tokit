//! Tests that prove the cache conformance kit: the four built-in caches pass every check, a
//! correct third-party queue passes too, and each deliberately-broken queue trips exactly the
//! check that owns its defect — with the two exceptions below, which are labelled rather than
//! left to look like the rest.
//!
//! The broken fixtures are one `Queue` type with a const-selected defect, so the defects sit side
//! by side and adding one is three lines rather than a new file.
//!
//! Two fixtures are not defects-under-test and say so at their definitions. `OVERFILL_TRIPWIRE`
//! asserts on the **kit's** driver rather than on a cache, pinning the fact that `peek` is
//! reached with a buffer that is not empty. `TOTAL_CAPACITY_PEEK` is a defect the kit provably
//! cannot see, and its test asserts the kit **accepts** it — an inverted test that keeps the
//! module docs' "what it deliberately does not check" section honest by failing if that ever
//! stops being true.

use core::cell::Cell;
use std::collections::VecDeque;

use mayber::Maybe;

use super::cache::CacheHarness;
use crate::{
  Lexer, Span, Token,
  cache::{
    Cache, CachedToken, CachedTokenOf, CachedTokenRefOf, DefaultCache, MaybeRefCachedTokenOf,
  },
  error::token::UnexpectedToken,
  lexer::LogosLexer,
};

// ── The corpus lexer: one-letter words, so every capacity fills and then refuses ─────

#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, skip r"[ \t]+")]
enum CTok {
  #[regex(r"[a-z]+")]
  Word,
}

impl core::fmt::Display for CTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("word")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CKind;

impl core::fmt::Display for CKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("word")
  }
}

#[derive(Debug, Clone, PartialEq)]
enum CErr {
  Any,
}

impl From<()> for CErr {
  fn from(_: ()) -> Self {
    CErr::Any
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for CErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    CErr::Any
  }
}

impl Token<'_> for CTok {
  type Kind = CKind;
  type Error = CErr;

  fn kind(&self) -> CKind {
    CKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type CLex<'a> = LogosLexer<'a, CTok>;

/// The corpus every cell runs over: twelve items — the widest capacity the cells use (8) plus a
/// full 4-slot peek window behind it. The capacity is what makes every cache here fill and then
/// refuse; the window is what the kit's peek prefill draws on, at every depth up to a buffer with
/// no room left, from tokens the cache under test is not itself holding.
const SRC: &str = "a b c d e f g h i j k l";

// ── The built-ins ───────────────────────────────────────────────────────────────────

#[test]
fn cache_kit_accepts_the_default_ring() {
  CacheHarness::<CLex<'_>, DefaultCache<'_, CLex<'_>>>::new(SRC)
    .named("DefaultCache (U3)")
    .run();
}

#[test]
fn cache_kit_accepts_a_wider_ring() {
  use generic_arraydeque::{GenericArrayDeque, typenum::U8};
  CacheHarness::<CLex<'_>, GenericArrayDeque<CachedTokenOf<'_, CLex<'_>>, U8>>::new(SRC)
    .named("GenericArrayDeque<_, U8>")
    .run();
}

#[test]
fn cache_kit_accepts_the_capacity_one_cache() {
  CacheHarness::<CLex<'_>, Option<CachedTokenOf<'_, CLex<'_>>>>::new(SRC)
    .named("Option (capacity 1)")
    .run();
}

#[test]
fn cache_kit_accepts_the_blackhole() {
  CacheHarness::<CLex<'_>, ()>::new(SRC)
    .named("() (capacity 0)")
    .run();
}

// ── A third-party queue, correct and broken ─────────────────────────────────────────

/// Defect selectors for [`Queue`]. `SOUND` is the deliberately-literal witness queue —
/// the shape whose `Cache::rewind` could not be written correctly from the method's own inputs,
/// and which now conforms because the method it could not implement no longer exists.
const SOUND: u8 = 0;
/// Declares `RETAINS_FRONT` and then refuses a front push into an empty cache.
const LYING_RETAINS_FRONT: u8 = 1;
/// `len()` under-reports by one, so `len`/`remaining` disagree with residency.
const SHORT_LEN: u8 = 2;
/// `pop_front` removes the newest entry — a LIFO stack wearing a FIFO name.
const LIFO_POP_FRONT: u8 = 3;
/// `pop_back` removes the oldest — the half the input layer's restore path is built on.
const FIFO_POP_BACK: u8 = 4;
/// `peek` appends newest-first.
const REVERSED_PEEK: u8 = 5;
/// `peek` answers differently on its second call: not logically pure.
const IMPURE_PEEK: u8 = 6;
/// `push_front` appends to the back instead of prepending.
const APPENDING_PUSH_FRONT: u8 = 7;
/// A refused push hands back a *different* token than the one offered.
const SWAPPING_REFUSAL: u8 = 8;
/// `peek` writes over the entries the destination buffer already held instead of appending
/// behind them — the buffer read as an output slot to fill rather than a queue to extend.
/// Invisible against an empty buffer, where clearing it is a no-op.
const CLOBBERING_PEEK: u8 = 9;
/// `peek` bounds itself by the destination buffer's TOTAL capacity rather than what `push_back`
/// can still accept (its REMAINING capacity), and discards the refused surplus in silence, as a
/// real cache written against the wrong bound would.
///
/// **The kit cannot see this**, and [`cache_kit_cannot_see_a_silent_total_capacity_peek`] asserts
/// so. It is here as the executable form of that limitation, not as a defect under test.
const TOTAL_CAPACITY_PEEK: u8 = 10;
/// Not a defect: a tripwire on the **kit's own driver**. `peek` bounds itself by the buffer's
/// total capacity and then asserts that every push it makes was accepted, which can only fail
/// where the kit hands `peek` a buffer with less room left than it has slots.
const OVERFILL_TRIPWIRE: u8 = 11;
/// `peek` answers correctly but drops an entry from its own residency — on the **non-empty
/// buffer path only**. Latched through a `Cell` rather than an actual drain, since a `VecDeque`
/// behind `&self` cannot be drained; `len()` is where the kit sees residency either way.
const PREFILL_DRAINS_RESIDENCY: u8 = 12;
/// `peek` is pure against an empty buffer and impure against a prefilled one: the first
/// non-empty-buffer call latches, and every call after it appends one entry fewer.
const PREFILL_IMPURE_PEEK: u8 = 13;
/// Refuses the **first-ever** `push_front` into a fresh cache and accepts one afterwards — a
/// front slot a `push_back` has to establish first. Declares `RETAINS_FRONT`.
const COLD_START_PUSH_FRONT: u8 = 14;
/// Accepts one `push_front` and refuses every later one **while the cache still has room** — a
/// refusal a full cache would have earned and this one has not.
///
/// It satisfies everything a refusal is otherwise asked for: the token comes back unchanged, the
/// residency does not move, and one front push *was* accepted, so the prepend law is observed
/// once. Only "a refusal means the cache is FULL" separates it from a conforming cache.
const EARLY_REFUSING_PUSH_FRONT: u8 = 15;
/// [`CLOBBERING_PEEK`] keyed on the prefix depth: `peek` appends correctly behind a **one-entry**
/// prefix and clobbers behind any deeper one.
///
/// A prefilled driver pinned at a single depth of 1 cannot tell this from a conforming cache —
/// which is what it was, so this fixture is the executable form of that gap.
const DEEP_PREFILL_CLOBBERING_PEEK: u8 = 16;
/// [`CLOBBERING_PEEK`] keyed on the other end of the depth sweep: `peek` conforms while the
/// buffer has room and clobbers when it has **none**.
///
/// The full buffer is the only shape in which the bound `min(len, remaining)` is asked to come
/// out zero, and the only one in which "append nothing" and "append what fits" stop being the
/// same instruction. This fixture is what keeps the top of the sweep from being a depth nothing
/// would miss.
const FULL_BUFFER_CLOBBERING_PEEK: u8 = 17;
/// `push_front` places its token at the front and **permutes what is behind it**: the two entries
/// after the head swap places, and only where the second of them is not the tail, so neither end
/// of the queue ever moves.
///
/// Every observable the kit reads at a *point* is correct after each push — the front is the
/// token just pushed, the back is the one `push_back` left there, `len` and `remaining` count —
/// so at capacity 4 it builds `[d,b,c,a]` where `[d,c,b,a]` was promised and says nothing about
/// it. Only an oracle that reads the WHOLE resident sequence separates it from a conforming
/// cache.
const REORDERING_PUSH_FRONT: u8 = 18;
/// `peek` bounds itself by the cache's BACKING store rather than by its current length, and what
/// it reads past the live run are entries the cache has already handed out.
///
/// Modelled with a graveyard that `pop_front` fills, since a `VecDeque` that really removes its
/// front leaves no stale slot to read: a fixed-array or ring cache does not remove, it moves an
/// index, and a `peek` that walks the array rather than the live run reads what the index moved
/// past. The bound `min(capacity, room)` equals the correct `min(len, room)` at exactly one
/// residency — a full cache — which is the only one `filled` alone ever builds.
const STALE_RESIDENCY_PEEK: u8 = 19;
/// Refuses a `push_front` into an **empty** cache, with every slot free, and accepts one as soon
/// as the cache holds anything. Declares `RETAINS_FRONT = false`.
///
/// The declaration is the whole of its cover. Check 2 returns at its first line for a cache that
/// declares `false`, and check 5 seeds the cache with a `push_back` before its first front push,
/// so the empty cache is never the state a front push meets there. The refusal is a violation
/// whatever the declaration says — a push is refused only by a FULL cache — and declaring `false`
/// changes only what the refusal costs the input layer.
const EMPTY_REFUSING_PUSH_FRONT: u8 = 20;
/// [`STALE_RESIDENCY_PEEK`] at the other end of the queue: `peek` is again bounded by the cache's
/// BACKING store rather than by its current length, but what it runs past the live run into are
/// the entries **`pop_back`** handed out.
///
/// This is the restore path's shape, and that is why it is worth its own cell. The input layer
/// drops an abandoned continuation's entries with a run of `pop_back` calls, so a ring or array
/// cache that moves its tail index back over slots it does not clear serves that abandoned
/// lookahead to the next `peek` — on a live rollback, not in a state nothing drives.
///
/// A residency sweep that only pops the FRONT cannot see it. Nothing fills the graveyard on that
/// path under this cell, so the bound `min(cap, room)` is applied to a store holding only the
/// shorter live run, `take` saturates on it, and the count and order that come out are the ones a
/// conforming cache would land. Only a state reached by `pop_back` puts entries behind the live
/// run for the wrong bound to reach.
///
/// Modelled with a graveyard for the same reason the front one is: a `VecDeque` that really
/// removes leaves no stale slot to read, so the defect could not exist in it at all.
const STALE_TAIL_RESIDENCY_PEEK: u8 = 21;
/// Not a defect: `peek` pushes `Maybe::Owned` clones instead of `Maybe::Ref` borrows — the
/// OWNED arm of the trait's contract, which `Cache::peek`'s own doc allows ("or an owned token,
/// if cache implementation requires") and which no built-in cache and no fixture before this one
/// ever took (#186). Every other observable is `SOUND`'s, so this proves the kit's existing
/// bound/order/purity/`peek_one`/prefill-sweep oracle certifies the owned arm exactly as it does
/// the borrowed one — it reads `.span()`/`.token()` through [`PeekedTokenExt`](crate::cache::PeekedTokenExt),
/// which is arm-blind by construction, so nothing about the oracle itself needed to change.
const OWNED_PEEK: u8 = 22;
/// [`OWNED_PEEK`] with a defect specific to that arm: every `Maybe::Owned` entry it pushes is a
/// clone of the FRONT token rather than the token at its own position, so the first entry of a
/// peek is right by coincidence — the front IS the first expected entry — and every entry behind
/// it is wrong. `front`, `back`, `len` and a hypothetical `Maybe::Ref` peek over the same data
/// would all still answer correctly; only the OWNED arm itself diverges, which is exactly the
/// shape the module docs name as invisible before #186: "an implementation whose owned arm is
/// wrong ... is certified by the kit regardless."
const WRONG_OWNED_PEEK: u8 = 23;
/// `is_empty()` returns `true` unconditionally, independent of `len()` (#180 part A, item 1).
///
/// The `Cache` trait's own `is_empty` is a DEFAULT method computed as `len() == 0`, so a fixture
/// that does not override it — every one above this line, [`OWNED_PEEK`] and
/// [`WRONG_OWNED_PEEK`] included — gets a correct answer for free, derived from a `len()` the kit
/// already checks exhaustively. This cell overrides it, which is the only way to give the kit
/// anything to be right or wrong ABOUT: before this cell existed, `cache.is_empty()` was called
/// from exactly one place in the kit (`assert_empty`), and only ever against a cache `len()` had
/// already established was empty — so a constant-`true` override was indistinguishable from a
/// correct one, at every capacity, in every existing test.
const LYING_IS_EMPTY: u8 = 24;
/// `span()` names the back end of the LAST-`push_back`'d token, forever — `pop_back` does not
/// invalidate it (#180 part A, item 3). `front()`/`back()`/`len()` stay correct throughout;
/// only the combined `span()` diverges, and only after a `pop_back`. Before this issue's fix,
/// `check_span` called `span()` exactly once per `run()`, immediately after a full `filled()` —
/// a residency this defect is correct at, since nothing has popped anything yet — so nothing
/// before this cell could tell it apart from a conforming cache.
const STALE_SPAN_AFTER_POP_BACK: u8 = 25;
/// `clear()` genuinely empties the cache — every check-1 observable is correct immediately
/// after it — but also poisons it: every `push_back` after a `clear()` is refused, with room to
/// spare, forever (#180 part A, item 2). Before this issue's fix nothing after `check_clear`
/// ever tried to use the cache again, so a `clear` that empties the cache and permanently
/// disables it passed exactly like one that only empties it.
const POISONING_CLEAR: u8 = 26;
/// `pop_front_if` removes the front regardless of what the predicate answers, and always
/// reports `None` — silently dropping a token whose predicate said to leave alone (#180 part A,
/// item 5). Nothing before this issue's fix ever called `pop_front_if`, so an override could
/// diverge from the default's `front` + `pop_front` composition in any way at all unnoticed.
const WRONG_POP_FRONT_IF: u8 = 27;
/// [`WRONG_POP_FRONT_IF`]'s sibling for `try_pop_front_if`: removes the front regardless of
/// what the predicate answers (even `Err`), and always reports `None` (#180 part A, item 5).
const WRONG_TRY_POP_FRONT_IF: u8 = 28;
/// `push_many` silently drops whatever does not fit instead of handing it back through the
/// overflow iterator (#180 part A, item 6). Nothing before this issue's fix ever called
/// `push_many`, so an override that discards tokens outright passed unnoticed.
const SILENT_PUSH_MANY: u8 = 29;
/// `clear()` empties the cache in every observable the kit reads at a point — `len`, `is_empty`,
/// `remaining`, `front`/`back`, the three span accessors and `peek` all answer for an empty
/// cache — but it **stashes the back entry**, and the first `pop_front()` after it hands that
/// entry back. A `clear` that unlinks the run without dropping it, and a `pop` that reads the
/// old link, is the natural shape of it.
///
/// This is the mutant [`POISONING_CLEAR`]'s sibling fix shipped without. `assert_empty` used to
/// run its pop probe against a throwaway `C::new()` rather than against the cache under test, so
/// "after clear() a pop must not answer" was asked of a cache that had never been cleared —
/// which is no question at all. The probe now runs on the cleared cache itself, and this cell is
/// what makes that change bite: a fresh `C::new()` has nothing stashed, so before the fix this
/// defect was certified in full (#180 part A, item 2).
const RESURRECTING_CLEAR: u8 = 30;
/// `peek_one` names the **back** entry instead of the front, with `peek`, `front`, `back`, `len`
/// and every span accessor left correct (#180 part A, item 4).
///
/// `peek_one` is a DEFAULT method — `peek::<U1>` into a one-slot buffer, then `pop_front` — so a
/// fixture that does not override it is correct wherever `peek` is, and every cell above this
/// line was. Nothing had ever overridden it, so the kit's `peek_one` assertion had no mutant
/// behind it at all: it was an untested assertion, which is the defect class this issue exists to
/// close. This cell is the witness that it bites. It answers correctly at `len <= 1`, where front
/// and back are the same entry, so only a residency the kit drives with two or more entries
/// separates it from a conforming cache.
const WRONG_PEEK_ONE: u8 = 31;
/// `peek_one` answers correctly the first time it is asked about a residency and answers `None`
/// every time it is asked again about the **same, unchanged** cache — the `peek_one`-specific
/// analogue of [`IMPURE_PEEK`] (#180 part A, item 4).
///
/// Keyed on the residency rather than on a raw call count, so it stays a statement about
/// repeatability alone: any mutation between two calls re-arms it, and only a second call with
/// nothing changed in between is answered wrongly. Before this issue's fix `peek_one` was called
/// exactly once per residency the kit visits, so a second answer had nothing to disagree with.
const IMPURE_PEEK_ONE: u8 = 32;
/// `front()` hands back the **back** entry while `front_span()` still names the front one
/// (#180 part A, item 8).
///
/// `front_span` is a DEFAULT method — `self.front().map(|t| t.token().span())` — so a cache that
/// gets `front` wrong and leaves `front_span` alone is caught by the span check the kit has
/// always run. This cell overrides **both**, which is what the gap actually needs: `front_span`
/// is exactly the accessor a ring specializes off its head index rather than paying to build a
/// `CachedTokenRef`, and once it is specialized the two can disagree. Everything the kit read
/// outside the empty state was the span half, so the reference half — the one that carries the
/// token and the `L::State` a restore resumes from — was checked for presence and nothing else.
/// Correct at `len <= 1`, where front and back are the same entry.
const WRONG_FRONT_IDENTITY: u8 = 33;
/// [`WRONG_FRONT_IDENTITY`] at the other end: `back()` hands back the **front** entry while
/// `back_span()` still names the back one (#180 part A, item 8).
const WRONG_BACK_IDENTITY: u8 = 34;

/// A `VecDeque`-backed third-party cache with a const-selected defect.
struct Queue<'a, L, const D: u8>
where
  L: Lexer<'a>,
{
  items: VecDeque<CachedTokenOf<'a, L>>,
  /// Entries a pop has already handed out and the store still holds — the slots a ring cache does
  /// not clear when an index moves past them. Filled by `pop_front` under
  /// [`STALE_RESIDENCY_PEEK`] and by `pop_back` under [`STALE_TAIL_RESIDENCY_PEEK`], and in both
  /// cases ordered as a walk from the head would meet them — behind the live run — so that
  /// `items` chained with it reads as the backing store a ring walks past its own length.
  graveyard: VecDeque<CachedTokenOf<'a, L>>,
  cap: usize,
  peeks: Cell<usize>,
  /// Set by the first `peek` that is handed a buffer which already holds entries.
  prefilled: Cell<bool>,
  /// Set by the first `push_back`.
  warmed: Cell<bool>,
  /// Counts the `push_front` calls this cache has accepted.
  front_pushes: Cell<usize>,
  /// The span of the most recent `push_back`, updated on every accepted one and never cleared
  /// or refreshed by a pop. Read only by [`STALE_SPAN_AFTER_POP_BACK`]'s `span()` override,
  /// which uses it as the combined span's end — correct up to the first `pop_back`, stale after.
  last_pushed_back_span: Option<L::Span>,
  /// Set by `clear()` under [`POISONING_CLEAR`] and never unset — every `push_back` after that
  /// checks it and refuses, regardless of capacity.
  poisoned: bool,
  /// The entry `clear()` stashed under [`RESURRECTING_CLEAR`] — read (and taken) by the first
  /// `pop_front` after that clear, and by nothing else. `items` is genuinely emptied, so every
  /// observable but the pop answers for an empty cache.
  stashed: Option<CachedTokenOf<'a, L>>,
  /// The residency the previous [`IMPURE_PEEK_ONE`] `peek_one` call was asked about. A call that
  /// finds the same value here is a **repeat** against an unchanged cache, which is the only call
  /// that cell answers wrongly.
  peek_one_at_len: Cell<Option<usize>>,
}

// `L::Token: Clone` is what the two stale-residency cells cost: `STALE_RESIDENCY_PEEK`'s
// `pop_front` and `STALE_TAIL_RESIDENCY_PEEK`'s `pop_back` each keep a copy of the entry they hand
// back, so that `peek` has something stale to read. `L::Span` and `L::State` are already `Clone`
// from `Lexer`/`State`, so this is the whole of the extra bound.
impl<'a, L, Lang: ?Sized, const D: u8> Cache<'a, L, Lang> for Queue<'a, L, D>
where
  L: Lexer<'a>,
  L::Token: Clone,
{
  type Options = usize;

  const RETAINS_FRONT: bool = D != APPENDING_PUSH_FRONT && D != EMPTY_REFUSING_PUSH_FRONT;

  fn new() -> Self {
    <Self as Cache<'a, L, Lang>>::with_options(4)
  }

  fn with_options(cap: usize) -> Self {
    Self {
      items: VecDeque::with_capacity(cap),
      graveyard: VecDeque::new(),
      cap,
      peeks: Cell::new(0),
      prefilled: Cell::new(false),
      warmed: Cell::new(false),
      front_pushes: Cell::new(0),
      last_pushed_back_span: None,
      poisoned: false,
      stashed: None,
      peek_one_at_len: Cell::new(None),
    }
  }

  fn len(&self) -> usize {
    if D == SHORT_LEN {
      return self.items.len().saturating_sub(1);
    }
    if D == PREFILL_DRAINS_RESIDENCY && self.prefilled.get() {
      return self.items.len().saturating_sub(1);
    }
    self.items.len()
  }

  fn remaining(&self) -> usize {
    self.cap - self.items.len()
  }

  fn is_empty(&self) -> bool {
    if D == LYING_IS_EMPTY {
      return true;
    }
    self.items.is_empty()
  }

  fn push_front(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
    if D == LYING_RETAINS_FRONT || self.items.len() == self.cap {
      return Err(self.refuse(tok));
    }
    if D == COLD_START_PUSH_FRONT && !self.warmed.get() {
      return Err(self.refuse(tok));
    }
    // A refusal by an EMPTY cache with every slot free — reached only after the full check
    // above, so nothing about the refusal's shape is wrong; what is missing is the fullness that
    // would have warranted it, at the one state check 5 has to seed its way past.
    if D == EMPTY_REFUSING_PUSH_FRONT && self.items.is_empty() {
      return Err(self.refuse(tok));
    }
    // A refusal with room to spare. Reached only after the full check above, so the *shape* of
    // the refusal — token back unchanged, residency untouched — is a conforming one; what is
    // missing is the fullness that would have warranted it.
    if D == EARLY_REFUSING_PUSH_FRONT && self.front_pushes.get() > 0 {
      return Err(self.refuse(tok));
    }
    self.front_pushes.set(self.front_pushes.get() + 1);
    if D == APPENDING_PUSH_FRONT {
      self.items.push_back(tok);
      return Ok(self.items.back().expect("just pushed").as_ref());
    }
    self.items.push_front(tok);
    if D == REORDERING_PUSH_FRONT && self.items.len() > 3 {
      // The interior, and only the interior: index 2 is the tail while there are three resident,
      // so the swap waits for a fourth. Neither the head this push just established nor the tail
      // `push_back` left there ever moves, which is what makes every point observable the kit
      // reads — front, back, len, remaining — agree with a conforming cache.
      self.items.swap(1, 2);
    }
    Ok(self.items.front().expect("just pushed").as_ref())
  }

  fn push_back(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
    self.push_back_impl(tok)
  }

  fn pop_front(&mut self) -> Option<CachedTokenOf<'a, L>> {
    self.pop_front_impl()
  }

  fn pop_back(&mut self) -> Option<CachedTokenOf<'a, L>> {
    if D == FIFO_POP_BACK {
      return self.items.pop_front();
    }
    let popped = self.items.pop_back();
    if D == STALE_TAIL_RESIDENCY_PEEK {
      // The newest entry leaves the live run and stays in the store — a ring's tail index moving
      // back over a slot it does not clear, which is what the input layer's restore path drives.
      // Pushed to the FRONT of the graveyard because the pops arrive newest-first: this keeps the
      // graveyard in append order, so `items` chained with it is the run a ring reads from its
      // head when the bound lets it walk past the live length. Nothing but `peek` reads it.
      if let Some(tok) = popped.as_ref() {
        self.graveyard.push_front(tok.clone());
      }
    }
    popped
  }

  fn pop_front_if<F>(&mut self, predicate: F) -> Option<CachedTokenOf<'a, L>>
  where
    F: FnOnce(CachedTokenRefOf<'_, 'a, L>) -> bool,
  {
    if D == WRONG_POP_FRONT_IF {
      // Removes the front regardless of what the predicate answers — even a false one — and
      // always reports None, silently dropping a token the caller's predicate said to leave
      // alone. The predicate still runs, so this looks conforming to anything that does not
      // read the return value.
      if let Some(peeked) = self.items.front().map(CachedToken::as_ref) {
        let _ = predicate(peeked);
        self.pop_front_impl();
      }
      return None;
    }
    if let Some(peeked) = self.items.front().map(CachedToken::as_ref) {
      if predicate(peeked) {
        return self.pop_front_impl();
      }
    }
    None
  }

  fn try_pop_front_if<E, F>(&mut self, predicate: F) -> Option<Result<CachedTokenOf<'a, L>, E>>
  where
    F: FnOnce(CachedTokenRefOf<'_, 'a, L>) -> Result<(), E>,
  {
    if D == WRONG_TRY_POP_FRONT_IF {
      // [`WRONG_POP_FRONT_IF`]'s sibling: removes the front regardless of what the predicate
      // answers, including `Err`, and always reports `None`.
      if let Some(peeked) = self.items.front().map(CachedToken::as_ref) {
        let _ = predicate(peeked);
        self.pop_front_impl();
      }
      return None;
    }
    if let Some(peeked) = self.items.front().map(CachedToken::as_ref) {
      return match predicate(peeked) {
        Ok(()) => self.pop_front_impl().map(Ok),
        Err(e) => Some(Err(e)),
      };
    }
    None
  }

  fn push_many<'p>(
    &'p mut self,
    toks: impl Iterator<Item = CachedTokenOf<'a, L>> + 'p,
  ) -> impl Iterator<Item = CachedTokenOf<'a, L>> + 'p {
    toks.filter_map(move |tok| {
      let refused = self.push_back_impl(tok).err();
      if D == SILENT_PUSH_MANY {
        // The refused token is dropped instead of handed back through the overflow iterator —
        // `push_back` above still ran, so residency up to capacity is unaffected; only the
        // overflow report is silently swallowed.
        None
      } else {
        refused
      }
    })
  }

  fn clear(&mut self) {
    if D == RESURRECTING_CLEAR {
      // Unlink the run without dropping it: `items` really is emptied, so `len`, `is_empty`,
      // `remaining`, `front`/`back`, the spans and `peek` all answer for an empty cache — but the
      // newest entry survives in `stashed`, and the first `pop_front` after this hands it back.
      self.stashed = self.items.pop_back();
    }
    self.items.clear();
    self.graveyard.clear();
    if D == POISONING_CLEAR {
      // The cache is genuinely empty right after this — every check-1 observable answers
      // correctly — but `push_back` refuses everything from here on regardless of capacity.
      self.poisoned = true;
    }
  }

  fn peek<'p, W>(
    &'p self,
    buf: &mut generic_arraydeque::GenericArrayDeque<MaybeRefCachedTokenOf<'p, 'a, L>, W::CAPACITY>,
  ) where
    W: crate::Window,
  {
    let seen = self.peeks.get();
    self.peeks.set(seen + 1);
    // Read before the latch is armed, so the FIRST non-empty-buffer call still answers
    // correctly and only the calls after it do not: a defect that needs the prefilled path to be
    // driven more than once to show, which is why the kit now drives it twice.
    let tainted = self.prefilled.get();
    if !buf.is_empty() {
      self.prefilled.set(true);
    }
    let mut fill = buf.remaining_capacity().min(self.items.len());
    if D == IMPURE_PEEK && seen > 0 {
      fill = fill.saturating_sub(1);
    }
    if D == PREFILL_IMPURE_PEEK && tainted {
      fill = fill.saturating_sub(1);
    }
    if D == REVERSED_PEEK {
      for tok in self.items.iter().rev().take(fill) {
        buf.push_back(Maybe::Ref(tok.as_ref()));
      }
      return;
    }
    if D == CLOBBERING_PEEK
      || (D == DEEP_PREFILL_CLOBBERING_PEEK && buf.len() > 1)
      || (D == FULL_BUFFER_CLOBBERING_PEEK && buf.remaining_capacity() == 0)
    {
      // The defect: the destination is read as an output buffer to fill rather than a queue to
      // append behind. Against an empty buffer `clear` is a no-op and the run that follows is
      // byte-for-byte a correct `peek`, so only a prefilled buffer can see it — and it reports
      // nothing about itself: the kit's own assertions are what catch it.
      //
      // Under `DEEP_PREFILL_CLOBBERING_PEEK` the same defect is gated on the prefix being deeper
      // than one entry, and under `FULL_BUFFER_CLOBBERING_PEEK` on the buffer having no room
      // left at all — one gate per end of the depth sweep, each invisible to a driver that does
      // not reach that end.
      buf.clear();
      let refill = buf.remaining_capacity().min(self.items.len());
      for tok in self.items.iter().take(refill) {
        buf.push_back(Maybe::Ref(tok.as_ref()));
      }
      return;
    }
    if D == STALE_RESIDENCY_PEEK || D == STALE_TAIL_RESIDENCY_PEEK {
      // The bound read off the backing store instead of the live run. `min(cap, room)` is
      // `min(len, room)` exactly while `len == cap`, so a cache the kit only ever fills answers
      // this correctly; below capacity it runs off the end of `items` and into the entries it
      // has already handed out, which is what a ring reading its array rather than its length
      // does. The count is what gives it away — the surplus is real lookahead to nobody.
      //
      // Which pop leaves those entries behind is the whole difference between the two cells, and
      // it is a difference in the DRIVER, not here: the graveyard is filled by `pop_front` under
      // `STALE_RESIDENCY_PEEK` and by `pop_back` under `STALE_TAIL_RESIDENCY_PEEK`, so each is
      // reachable only from the residency sweep that drains that end. Under the other sweep the
      // graveyard stays empty and `take` saturates on the live run, which is a conforming answer.
      let stale_fill = buf.remaining_capacity().min(self.cap);
      let store = self.items.iter().chain(self.graveyard.iter());
      for tok in store.take(stale_fill) {
        buf.push_back(Maybe::Ref(tok.as_ref()));
      }
      return;
    }
    if D == TOTAL_CAPACITY_PEEK {
      // Bounded by `buf.capacity()` (the buffer's TOTAL capacity) instead of
      // `buf.remaining_capacity()` (used everywhere else in this method), discarding the
      // surplus `push_back` refuses — the shape a real cache written against the wrong bound
      // has. `min(min(len, W), W - P) == min(len, W - P)`, and a refused `push_back` leaves the
      // deque untouched, so the entries this lands are exactly the entries a correct `peek`
      // would land, in every configuration. Nothing here or in the kit can tell the difference;
      // see `cache_kit_cannot_see_a_silent_total_capacity_peek`.
      let over_fill = buf.capacity().min(self.items.len());
      for tok in self.items.iter().take(over_fill) {
        let _ = buf.push_back(Maybe::Ref(tok.as_ref()));
      }
      return;
    }
    if D == OVERFILL_TRIPWIRE {
      // Not a cache defect under test — an assertion pointed the other way, at the kit. It
      // fires only if the kit hands `peek` a buffer whose remaining capacity is below its total
      // AND a residency long enough to run past what is left, which is exactly the driver shape
      // check 6 needs and which nothing in the kit's output would otherwise reveal.
      let remaining = buf.remaining_capacity();
      let over_fill = buf.capacity().min(self.items.len());
      for tok in self.items.iter().take(over_fill) {
        assert!(
          buf.push_back(Maybe::Ref(tok.as_ref())).is_none(),
          "conformance-kit driver tripwire: a peek bounded by the destination buffer's TOTAL capacity ({}) rather than its REMAINING capacity ({remaining}) ran past the room left, so the kit did hand `peek` a buffer that was not empty",
          buf.capacity()
        );
      }
      return;
    }
    if D == OWNED_PEEK {
      // The owned arm, otherwise correct: same bound, same order, same source — a clone of the
      // entry at its own position, not a borrow of it.
      for tok in self.items.iter().take(fill) {
        buf.push_back(Maybe::Owned(tok.clone()));
      }
      return;
    }
    if D == WRONG_OWNED_PEEK {
      // The owned arm, made wrong: every entry is a clone of the FRONT token regardless of its
      // own position. `front()` stays correct — this cell's whole point is that the owned peek
      // arm can diverge from every other observable, including a Ref-shaped peek over the same
      // data, without anything but a check that actually reads the owned arm noticing.
      //
      // Guarded on `fill > 0`: an empty cache (check 1) or a full buffer (bound 0) must still
      // push nothing, the same as a conforming `peek`, so `front()` is never asked to answer for
      // a cache that is not resident at all.
      if fill > 0 {
        let wrong = self
          .items
          .front()
          .expect("fill > 0 implies at least one resident entry")
          .clone();
        for _ in 0..fill {
          buf.push_back(Maybe::Owned(wrong.clone()));
        }
      }
      return;
    }
    for tok in self.items.iter().take(fill) {
      buf.push_back(Maybe::Ref(tok.as_ref()));
    }
  }

  fn peek_one<'c>(&self) -> Option<MaybeRefCachedTokenOf<'_, 'a, L>>
  where
    'a: 'c,
  {
    if D == WRONG_PEEK_ONE {
      // The BACK entry, not the front. Identical to a conforming answer at `len <= 1`, where the
      // two are the same entry; wrong at every deeper residency. `peek` is untouched, so a
      // `peek_one` derived from `peek` — the default — would still be right, which is what makes
      // this cell a statement about the override alone.
      return self.items.back().map(|tok| Maybe::Ref(tok.as_ref()));
    }
    if D == IMPURE_PEEK_ONE {
      let len = self.items.len();
      let repeat = self.peek_one_at_len.replace(Some(len)) == Some(len);
      if repeat {
        // A second call with nothing changed in between. `peek_one` takes `&self`, so the two
        // calls are the same question and must get the same answer.
        return None;
      }
    }
    self.items.front().map(|tok| Maybe::Ref(tok.as_ref()))
  }

  fn front(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
    if D == WRONG_FRONT_IDENTITY {
      // The wrong END, while `front_span` below keeps naming the right one.
      return self.items.back().map(CachedToken::as_ref);
    }
    self.items.front().map(CachedToken::as_ref)
  }

  fn back(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
    if D == WRONG_BACK_IDENTITY {
      return self.items.front().map(CachedToken::as_ref);
    }
    self.items.back().map(CachedToken::as_ref)
  }

  // `front_span`/`back_span` are DEFAULT methods composed from `front`/`back`, so without these
  // two overrides the cells above would break the span accessors too and be caught by a check
  // that has always existed. Specializing them — reading the head and tail entries directly,
  // which is what a ring does to avoid building a `CachedTokenRef` — is what lets the reference
  // half and the span half disagree, and that disagreement is the gap (#180 part A, item 8).
  fn front_span<'s>(&'s self) -> Option<&'s L::Span>
  where
    'a: 's,
  {
    self
      .items
      .front()
      .map(CachedToken::as_ref)
      .map(|t| *t.token().span())
  }

  fn back_span<'s>(&'s self) -> Option<&'s L::Span>
  where
    'a: 's,
  {
    self
      .items
      .back()
      .map(CachedToken::as_ref)
      .map(|t| *t.token().span())
  }

  fn span(&self) -> Option<L::Span> {
    if D == STALE_SPAN_AFTER_POP_BACK {
      let front = self.items.front()?;
      // The live front, paired with the end of the LAST push_back rather than the current
      // back — correct until the first pop_back, stale after, exactly like a cache that reads
      // `span()`'s end off a value nothing keeps in sync with `pop_back`.
      let end = self
        .last_pushed_back_span
        .as_ref()
        .expect("non-empty items implies at least one accepted push_back")
        .end();
      return Some(L::Span::new(front.token().span_ref().start(), end));
    }
    match (self.items.front(), self.items.back()) {
      (Some(first), Some(last)) => Some(L::Span::new(
        first.token().span_ref().start(),
        last.token().span_ref().end(),
      )),
      _ => None,
    }
  }
}

impl<'a, L, const D: u8> Queue<'a, L, D>
where
  L: Lexer<'a>,
  L::Token: Clone,
{
  /// The refusal round-trip — or, under `SWAPPING_REFUSAL`, its violation: the caller is handed a
  /// resident entry instead of the token it offered, which silently swallows that token.
  fn refuse(&mut self, tok: CachedTokenOf<'a, L>) -> CachedTokenOf<'a, L> {
    // Nested rather than a let-chain: the MSRV (1.87) does not have them.
    if D == SWAPPING_REFUSAL {
      if let Some(other) = self.items.pop_back() {
        self.items.push_back(tok);
        return other;
      }
    }
    tok
  }

  /// `Cache::push_back`'s real body, as an inherent method: `push_many`'s default composition
  /// (`toks.filter_map(move |tok| self.push_back(tok).err())`) calls it directly rather than
  /// through the trait, because `self.push_back(tok)` from inside another method of the SAME
  /// `impl<Lang> Cache<'a, L, Lang> for Queue` block cannot infer which `Lang` to dispatch
  /// through — nothing in any method's signature ties it down, since `Queue` implements
  /// `Cache<'a, L, Lang>` for every `Lang` at once. `front`/`back` sidestep the same issue by
  /// reading `self.items` directly, which works because their trait bodies do nothing else; this
  /// one cannot, since accepting or refusing a push is the behavior under test.
  fn push_back_impl(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
    if D == POISONING_CLEAR && self.poisoned {
      return Err(self.refuse(tok));
    }
    if self.items.len() == self.cap {
      return Err(self.refuse(tok));
    }
    self.warmed.set(true);
    // Read before the move below, for `STALE_SPAN_AFTER_POP_BACK`'s `span()` override — see
    // that constant's doc for why this is never cleared or refreshed by a pop.
    self.last_pushed_back_span = Some((*tok.token().span_ref()).clone());
    self.items.push_back(tok);
    Ok(self.items.back().expect("just pushed").as_ref())
  }

  /// `Cache::pop_front`'s real body, as an inherent method — for the same reason
  /// [`push_back_impl`](Self::push_back_impl) exists: `pop_front_if`'s trait-method override
  /// calls this rather than `self.pop_front()`, which cannot infer which `Lang` to dispatch
  /// through from inside another method of the same generic-over-`Lang` impl block.
  fn pop_front_impl(&mut self) -> Option<CachedTokenOf<'a, L>> {
    if D == LIFO_POP_FRONT {
      return self.items.pop_back();
    }
    // The entry `clear()` unlinked but did not drop, handed back by the first pop after it. Only
    // reachable while `items` is empty, so the resurrection never perturbs a live residency —
    // the whole point of the cell is that every other observable stays correct.
    if D == RESURRECTING_CLEAR && self.items.is_empty() {
      if let Some(tok) = self.stashed.take() {
        return Some(tok);
      }
    }
    let popped = self.items.pop_front();
    if D == STALE_RESIDENCY_PEEK {
      // The entry leaves the live run and stays in the store — a ring's head moving past a slot
      // it does not clear. Nothing but `peek` reads it, so `len`, `front`, `back` and `remaining`
      // all keep answering for the live run alone.
      if let Some(tok) = popped.as_ref() {
        self.graveyard.push_back(tok.clone());
      }
    }
    popped
  }
}

/// Runs the kit over `Queue<D>`, so each cell below is one line.
fn run_queue<const D: u8>() {
  CacheHarness::<CLex<'_>, Queue<'_, CLex<'_>, D>>::new(SRC)
    .named("third-party queue")
    .run();
}

#[test]
fn cache_kit_accepts_a_correct_third_party_queue() {
  run_queue::<SOUND>();
}

/// #186: the kit never drove `Cache::peek`'s OWNED arm — every built-in cache and every fixture
/// above pushes `Maybe::Ref`. `OWNED_PEEK` is `SOUND` in every other respect, so a pass here
/// proves the kit's bound/order/purity/`peek_one`/prefill-sweep oracle certifies the owned arm
/// on its own merits, not by accident of it being untested.
#[test]
fn cache_kit_accepts_a_queue_whose_peek_returns_owned_tokens() {
  run_queue::<OWNED_PEEK>();
}

/// #186: the owned-arm counterpart to [`cache_kit_catches_a_reversed_peek`] and friends — a
/// defect that exists ONLY on the arm [`cache_kit_accepts_a_queue_whose_peek_returns_owned_tokens`]
/// just proved the kit can certify. Before #186 nothing drove this arm at all, so nothing could
/// have caught this.
#[test]
#[should_panic(expected = "bounded-peek")]
fn cache_kit_catches_a_wrong_owned_peek() {
  run_queue::<WRONG_OWNED_PEEK>();
}

/// #180 part A, item 1: `is_empty()` is never asserted FALSE anywhere in the kit before this
/// fixture — `LYING_IS_EMPTY` returns `true` unconditionally and, before the fix this fixture
/// proves, that constant answer was certified by the kit at every capacity this suite drives.
#[test]
#[should_panic(expected = "empty-invariants")]
fn cache_kit_catches_a_lying_is_empty() {
  run_queue::<LYING_IS_EMPTY>();
}

/// #180 part A, item 3: `span()` was checked at exactly one residency (a full, never-popped
/// cache) — `STALE_SPAN_AFTER_POP_BACK` is correct there and wrong only after a `pop_back`,
/// which the check before this fixture never drove.
#[test]
#[should_panic(expected = "combined-span")]
fn cache_kit_catches_a_stale_span_after_pop_back() {
  run_queue::<STALE_SPAN_AFTER_POP_BACK>();
}

/// #180 part A, item 2: `clear()` was never reused afterward, so a `clear` that empties the
/// cache but also permanently disables future pushes passed exactly like a conforming one.
#[test]
#[should_panic(expected = "clear]")]
fn cache_kit_catches_a_poisoning_clear() {
  run_queue::<POISONING_CLEAR>();
}

/// #180 part A, item 2, second half — the one that shipped with no mutant behind it.
/// `assert_empty`'s pop probe used to run against a throwaway `C::new()`; it now runs against
/// the cache under test, which is the only way "after clear() a pop must not answer" is a
/// question about the cache that was cleared. `RESURRECTING_CLEAR` is the defect that change
/// catches and the unfixed probe cannot: `clear()` unlinks the run without dropping it, every
/// point observable answers for an empty cache, and the first `pop_front` after it hands the
/// stashed entry back. A fresh `C::new()` has nothing stashed, so it answers `None` and the old
/// probe was satisfied.
#[test]
#[should_panic(expected = "a pop answered on an empty cache")]
fn cache_kit_catches_a_resurrecting_clear() {
  run_queue::<RESURRECTING_CLEAR>();
}

/// #180 part A, item 4, first half: `peek_one` had no mutant at all. The assertion that it names
/// the front entry existed and had never been run against a cache that disagrees with it, because
/// `peek_one` is a default method and no fixture had ever overridden it. This is that mutant.
#[test]
#[should_panic(expected = "peek_one() named")]
fn cache_kit_catches_a_wrong_peek_one() {
  run_queue::<WRONG_PEEK_ONE>();
}

/// #180 part A, item 4, second half: `peek_one` was never called twice against one unchanged
/// cache, so an answer that is correct once and wrong on every repeat passed. `peek` has been
/// checked for exactly this since the kit existed ([`IMPURE_PEEK`]); `peek_one` had not.
#[test]
#[should_panic(expected = "pure-peek-one")]
fn cache_kit_catches_an_impure_peek_one() {
  run_queue::<IMPURE_PEEK_ONE>();
}

/// #180 part A, item 8: outside the empty state the kit read `front_span()`/`back_span()` and
/// never `front()`/`back()` themselves, so a cache whose specialized span accessors are right
/// and whose entry accessors are wrong was certified. The entry is what carries the token and
/// the `L::State` a restore resumes from.
#[test]
#[should_panic(expected = "front() names")]
fn cache_kit_catches_a_front_that_names_the_wrong_entry() {
  run_queue::<WRONG_FRONT_IDENTITY>();
}

/// [`cache_kit_catches_a_front_that_names_the_wrong_entry`] at the other end of the queue.
#[test]
#[should_panic(expected = "back() names")]
fn cache_kit_catches_a_back_that_names_the_wrong_entry() {
  run_queue::<WRONG_BACK_IDENTITY>();
}

/// #180 part A, item 5: `pop_front_if`/`try_pop_front_if` were never called by the kit, so an
/// override could remove on a false predicate and report `None` — silently dropping a token —
/// and pass exactly like a conforming default implementation. Caught here by the very first
/// sub-check (a false predicate must remove nothing), via `assert_resident`'s exact-length
/// assertion rather than one of this check's own — `pop_front_if` is matched rather than the
/// `pop-front-if` message tag so either path is accepted.
#[test]
#[should_panic(expected = "pop_front_if")]
fn cache_kit_catches_a_wrong_pop_front_if() {
  run_queue::<WRONG_POP_FRONT_IF>();
}

/// The `try_pop_front_if` sibling of [`cache_kit_catches_a_wrong_pop_front_if`], caught by this
/// check's own dedicated Err-predicate assertion instead (`try_pop_front_if` is a substring of
/// that message too, so the same `expected` pattern matches).
#[test]
#[should_panic(expected = "pop_front_if")]
fn cache_kit_catches_a_wrong_try_pop_front_if() {
  run_queue::<WRONG_TRY_POP_FRONT_IF>();
}

/// #180 part A, item 6: `push_many` was never called by the kit, so an override that silently
/// discards whatever does not fit — instead of handing it back through the overflow iterator —
/// passed exactly like a conforming default implementation.
#[test]
#[should_panic(expected = "push-many")]
fn cache_kit_catches_a_silent_push_many() {
  run_queue::<SILENT_PUSH_MANY>();
}

#[test]
#[should_panic(expected = "retains-front")]
fn cache_kit_catches_a_lying_retains_front() {
  run_queue::<LYING_RETAINS_FRONT>();
}

#[test]
#[should_panic(expected = "exact-length")]
fn cache_kit_catches_an_inexact_len() {
  run_queue::<SHORT_LEN>();
}

#[test]
#[should_panic(expected = "order")]
fn cache_kit_catches_a_lifo_pop_front() {
  run_queue::<LIFO_POP_FRONT>();
}

#[test]
#[should_panic(expected = "order")]
fn cache_kit_catches_a_fifo_pop_back() {
  run_queue::<FIFO_POP_BACK>();
}

#[test]
#[should_panic(expected = "bounded-peek")]
fn cache_kit_catches_a_reversed_peek() {
  run_queue::<REVERSED_PEEK>();
}

#[test]
#[should_panic(expected = "pure-peek")]
fn cache_kit_catches_an_impure_peek() {
  run_queue::<IMPURE_PEEK>();
}

#[test]
#[should_panic(expected = "push-front")]
fn cache_kit_catches_an_appending_push_front() {
  run_queue::<APPENDING_PUSH_FRONT>();
}

#[test]
#[should_panic(expected = "refusal-round-trip")]
fn cache_kit_catches_a_swapping_refusal() {
  run_queue::<SWAPPING_REFUSAL>();
}

#[test]
#[should_panic(expected = "bounded-peek/prefilled")]
fn cache_kit_catches_a_clobbering_peek() {
  run_queue::<CLOBBERING_PEEK>();
}

/// The kit's driver, not a cache, is what this pins: `peek` must be called at least once with a
/// buffer whose remaining capacity is below its total and a residency long enough to overrun it.
///
/// The fixture arms an assertion inside its own `peek` that can only fire in that situation, so
/// this test failing means the kit went back to peeking exclusively into fresh, empty buffers —
/// the state in which the bound the trait states (`min(len, REMAINING capacity)`) and the bound
/// it does not (`min(len, TOTAL capacity)`) are the same number and check 6 evaluates both to
/// the same verdict.
#[test]
#[should_panic(expected = "driver tripwire")]
fn cache_kit_drives_peek_with_less_room_than_the_buffer_has_slots() {
  run_queue::<OVERFILL_TRIPWIRE>();
}

/// A defect the kit provably **cannot** catch, asserted as such so the claim stays honest.
///
/// `TOTAL_CAPACITY_PEEK` reads the bound off `buf.capacity()` instead of
/// `buf.remaining_capacity()` — the exact violation check 6 exists to reject — and then discards
/// what `push_back` refuses, which is what a cache written against the wrong bound actually
/// does. It passes every check, and must: with `W` the buffer's capacity and `P` what it already
/// holds, it lands `min(min(len, W), W - P)` entries, and that is `min(len, W - P)`, the correct
/// count, for every `len`, `W` and `P`. `GenericArrayDeque::push_back` returns the value and
/// leaves the deque unmodified when full, so the overflow attempts leave no trace either.
///
/// This test is therefore an inverted one: it passes while the kit is blind and fails the moment
/// the kit — or the deque's overflow behaviour underneath it — gains a way to see this. Either
/// way the module docs' "what it deliberately does not check" section needs revisiting, which is
/// why the failure is worth having.
#[test]
fn cache_kit_cannot_see_a_silent_total_capacity_peek() {
  run_queue::<TOTAL_CAPACITY_PEEK>();
}

#[test]
#[should_panic(expected = "after prefilled peek()")]
fn cache_kit_catches_a_peek_that_drains_only_on_the_prefilled_path() {
  run_queue::<PREFILL_DRAINS_RESIDENCY>();
}

#[test]
#[should_panic(expected = "pure-peek/prefilled")]
fn cache_kit_catches_a_peek_that_is_impure_only_on_the_prefilled_path() {
  run_queue::<PREFILL_IMPURE_PEEK>();
}

#[test]
#[should_panic(expected = "FIRST operation on a fresh cache")]
fn cache_kit_catches_a_cold_start_push_front() {
  run_queue::<COLD_START_PUSH_FRONT>();
}

/// A refusal is only ever warranted by a **full** cache, and that is checked on the `push_front`
/// arm as it always was on the `push_back` one.
///
/// The expected message names the *second* front push: the fixture accepts the first, so a check
/// that only asked whether some front push was ever accepted — which is all the closing
/// `resident.len() > 1` assertion asks — is satisfied and this one is not.
#[test]
#[should_panic(expected = "push_front #1 was REFUSED")]
fn cache_kit_catches_a_push_front_refused_with_room_to_spare() {
  run_queue::<EARLY_REFUSING_PUSH_FRONT>();
}

/// The prefilled peek is driven at every depth the window has room for, not only at one entry.
///
/// The expected message names **depth 2**: the fixture is conforming behind a one-entry prefix,
/// so this test failing means the kit went back to prefilling a single entry, where a
/// depth-sensitive `peek` and a correct one agree.
#[test]
#[should_panic(expected = "bounded-peek/prefilled at depth 2")]
fn cache_kit_catches_a_peek_that_clobbers_only_a_deeper_prefix() {
  run_queue::<DEEP_PREFILL_CLOBBERING_PEEK>();
}

/// The other end of the same sweep: the prefill reaches a buffer with **no room left**.
///
/// The expected message names depth 4, the peek window's full capacity. That depth is the only
/// one at which the bound is zero, so this test failing means the sweep stops short of a full
/// buffer and "append nothing" is no longer a case the kit puts to a cache.
#[test]
#[should_panic(expected = "bounded-peek/prefilled at depth 4")]
fn cache_kit_catches_a_peek_that_clobbers_a_buffer_with_no_room_left() {
  run_queue::<FULL_BUFFER_CLOBBERING_PEEK>();
}

/// The prepend law is read over the **whole** resident sequence, not at its two ends.
///
/// The expected message names **depth 3**: the fixture leaves the queue alone until there are
/// four resident, since below that the pair it swaps includes the tail. So this test failing
/// means the drain either stopped short of the deepest accepted prefix or went back to comparing
/// only `front`, `back` and `len` — every one of which this fixture answers correctly at every
/// step.
#[test]
#[should_panic(expected = "push-front/full-order at depth 3")]
fn cache_kit_catches_a_push_front_that_reorders_the_interior() {
  run_queue::<REORDERING_PUSH_FRONT>();
}

/// `peek` is driven at residencies **below** the capacity, not only at a cache that has been
/// filled and never popped.
///
/// The expected message names the first partly drained state. A cache filled to capacity is the
/// one residency where a bound read off the backing store and a bound read off the live run are
/// the same number, so this test failing means the sweep went back to that single state — where a
/// `peek` serving already-consumed entries is indistinguishable from a conforming one.
#[test]
#[should_panic(expected = "after 1 pop_front(s) off a full one")]
fn cache_kit_catches_a_peek_bounded_by_the_backing_store() {
  run_queue::<STALE_RESIDENCY_PEEK>();
}

/// The residency sweep drains from the **back** as well as the front — the restore path's shape.
///
/// The expected message names the first state reached with `pop_back`. This fixture is
/// conforming under every `pop_front` residency, since the entries it fails to clear sit behind
/// the live run and the bound saturates before reaching them, so this test failing means the
/// mirrored sweep is gone and the kit is back to asking about stale entries at one end of the
/// queue only — the end the input layer's rollback does *not* remove from.
#[test]
#[should_panic(expected = "after 1 pop_back(s) off a full one")]
fn cache_kit_catches_a_peek_bounded_by_the_backing_store_after_a_tail_drain() {
  run_queue::<STALE_TAIL_RESIDENCY_PEEK>();
}

/// The full-only refusal law is asserted at the empty cache for **every** nonzero capacity, not
/// only where `RETAINS_FRONT` is declared.
///
/// The fixture declares `false`, which is what kept it invisible: the declaration check returns
/// immediately for such a cache, and the prepend check seeds the cache with a `push_back` before
/// its first front push. This test failing means the queue law and the declaration have been
/// folded back together.
#[test]
#[should_panic(expected = "push-front/into-empty")]
fn cache_kit_catches_a_push_front_refused_by_an_empty_cache() {
  run_queue::<EMPTY_REFUSING_PUSH_FRONT>();
}
