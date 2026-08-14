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

use core::cell::{Cell, RefCell};
use std::collections::VecDeque;

use generic_arraydeque::typenum::Unsigned;
use mayber::Maybe;

use super::cache::CacheHarness;
use crate::{
  Lexer, Span, Token,
  cache::{
    Cache, CachedToken, CachedTokenOf, CachedTokenRefOf, DefaultCache, MaybeRefCachedTokenOf,
    PeekedTokenExt,
  },
  error::token::UnexpectedToken,
  lexer::LogosLexer,
  span::Spanned,
};

// ── The corpus lexer: alternating kinds, distinct payloads, an advancing state ────────

/// The corpus lexer's state: a counter every token callback bumps, so the `L::State` cached
/// beside each corpus token is a **different value at every position** (`lexed` runs 1..=12 over
/// [`SRC`]).
///
/// Before #183 this was `()` — logos' default `Extras`, which is what a lexer gets when it is not
/// given one — and a single-valued state makes the kit's state comparison vacuous: there is
/// nothing for a cache to re-associate wrongly. Every state mutant below rests on this counter,
/// and [`corpus_is_pairwise_distinct_on_all_three_axes`] is what stops a later `SRC` edit from
/// quietly taking it away again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CState {
  lexed: u32,
}

impl crate::State for CState {
  type Error = ();

  fn check(&self) -> Result<(), Self::Error> {
    Ok(())
  }
}

/// Two token kinds, **alternating** over [`SRC`], each carrying its own source slice.
///
/// Both halves matter, and they are different properties. Alternating *kinds* is what makes a
/// neighbour swap visible; distinct *payloads* are what make a swap between two same-kind entries
/// at a distance visible, since the payload is the slice and no two positions share one. Before
/// #183 this enum was a single fieldless variant — one inhabitant — so a token comparison over it
/// could not have failed even if the kit had made one.
#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, skip r"[ \t]+", extras = CState)]
enum CTok<'a> {
  #[regex(r"[a-z]+", |lex| { lex.extras.lexed += 1; lex.slice() })]
  Word(&'a str),
  #[regex(r"[0-9]+", |lex| { lex.extras.lexed += 1; lex.slice() })]
  Num(&'a str),
}

impl core::fmt::Display for CTok<'_> {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Word(s) => write!(f, "word {s}"),
      Self::Num(s) => write!(f, "num {s}"),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CKind {
  Word,
  Num,
}

impl core::fmt::Display for CKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Word => f.write_str("word"),
      Self::Num => f.write_str("num"),
    }
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

impl<'a> Token<'a> for CTok<'a> {
  type Kind = CKind;
  type Error = CErr;

  const READ_FRONTIER_CLASS: crate::ReadFrontierClass = crate::ReadFrontierClass::Unbounded;

  fn kind(&self) -> CKind {
    match self {
      Self::Word(_) => CKind::Word,
      Self::Num(_) => CKind::Num,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type CLex<'a> = LogosLexer<'a, CTok<'a>>;

/// The corpus every cell runs over: twelve items — the widest capacity the cells use (8) plus a
/// full 4-slot peek window behind it. The capacity is what makes every cache here fill and then
/// refuse; the window is what the kit's peek prefill draws on, at every depth up to a buffer with
/// no room left, from tokens the cache under test is not itself holding.
///
/// **Heterogeneous on all three axes** since #183. Spans are pairwise distinct from any source;
/// the alternation of letters and digits makes the token *kinds* alternate, the slices make the
/// token *values* pairwise distinct, and [`CState`]'s counter makes the states pairwise distinct.
/// That is what gives the entry comparison anything to discriminate — see
/// [`corpus_is_pairwise_distinct_on_all_three_axes`], which fails if a later edit takes any of it
/// away.
const SRC: &str = "a 1 b 2 c 3 d 4 e 5 f 6";

// ── The built-ins ───────────────────────────────────────────────────────────────────

// Every built-in takes `Options = ()`, so `also_built_by` drives their second constructor with
// the only value that type has. Without it `Cache::with_options` was never called by the kit at
// all — not for a third-party cache and not for tokora's own (#180 part A, item 7).

#[test]
fn cache_kit_accepts_the_default_ring() {
  CacheHarness::<CLex<'_>, DefaultCache<'_, CLex<'_>>>::new(SRC)
    .named("DefaultCache (U3)")
    .also_built_by(|| <DefaultCache<'_, CLex<'_>> as Cache<'_, CLex<'_>, ()>>::with_options(()))
    .run();
}

#[test]
fn cache_kit_accepts_a_wider_ring() {
  use generic_arraydeque::{GenericArrayDeque, typenum::U8};
  type Wide<'a> = GenericArrayDeque<CachedTokenOf<'a, CLex<'a>>, U8>;
  CacheHarness::<CLex<'_>, Wide<'_>>::new(SRC)
    .named("GenericArrayDeque<_, U8>")
    .also_built_by(|| <Wide<'_> as Cache<'_, CLex<'_>, ()>>::with_options(()))
    .run();
}

#[test]
fn cache_kit_accepts_the_capacity_one_cache() {
  type One<'a> = Option<CachedTokenOf<'a, CLex<'a>>>;
  CacheHarness::<CLex<'_>, One<'_>>::new(SRC)
    .named("Option (capacity 1)")
    .also_built_by(|| <One<'_> as Cache<'_, CLex<'_>, ()>>::with_options(()))
    .run();
}

#[test]
fn cache_kit_accepts_the_blackhole() {
  CacheHarness::<CLex<'_>, ()>::new(SRC)
    .named("() (capacity 0)")
    .also_built_by(|| <() as Cache<'_, CLex<'_>, ()>>::with_options(()))
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
/// `peek` serves the front entry `bound` times over instead of walking the residency once — the
/// direct witness for check 6's "**each resident token once**" clause (#180 part A, item 9).
///
/// The clause had no fixture of its own. It is carried by the exact-sequence assertion
/// (`peek() appended {first:?}, expected the resident prefix OLDEST FIRST`) rather than by a
/// distinctness check of its own, and that assertion catches this cell unchanged — see
/// [`CacheHarness::check_peek_at`](super::cache) for why a separate distinctness assertion is
/// **not** added: with the corpus spans pairwise distinct, an appended run that equals the
/// resident prefix is distinct by construction, so such an assertion could not be made to fail.
/// [`WRONG_OWNED_PEEK`] is the same defect on the OWNED arm; this one is on the borrowed arm
/// every built-in cache actually takes.
///
/// Correct at every bound of 0 or 1, where "serve the front repeatedly" and "walk the residency
/// once" are the same instruction.
const DUPLICATING_PEEK: u8 = 35;
/// `Cache::new()` is conformant and `Cache::with_options(n)` builds a cache that **reports** a
/// capacity of `n` and can actually hold `n - 1` (#180 part A, item 7).
///
/// The kit built every cache it tested with `C::new()`, so the second constructor was never
/// called at all and a cache could get it arbitrarily wrong. This cell is the shape the issue
/// names — a `with_options` that hands back the wrong capacity — and it is a defect the kit can
/// see only because the cache's own `remaining()` disagrees with how many `push_back`s it then
/// accepts. The kit cannot check that the capacity matches what the CALLER asked for: `Options`
/// is an opaque associated type, so `with_options(6)` returning a capacity-3 cache is
/// internally consistent and conforming as far as any generic oracle can tell.
///
/// Reached through [`CacheHarness::also_built_by`](super::cache::CacheHarness::also_built_by),
/// so the failure it produces carries the `built by with_options` tag and cannot be confused
/// with a failure on the `C::new()` pass.
const WRONG_WITH_OPTIONS: u8 = 36;
/// A refused `push_front` hands back a **resident** entry instead of the token it was offered,
/// and keeps the offered one — but only at capacity 1 (#180 part A, item 10).
///
/// [`SWAPPING_REFUSAL`] is the same violation without the capacity gate, and check 5's
/// round-trip catches it at any capacity of 2 or more. The gate is what makes this cell a
/// statement about capacity 1 alone, which `check_push_front` returned at its first line for and
/// never drove: the prepend ORDER is not observable there — the seeding `push_back` fills the
/// cache, so no front push can be accepted — but the refusal half is, and nothing else in the
/// kit reaches it on this arm. Check 3 drives the round-trip for `push_back`; check 2 and the
/// empty-cache front-push check only ever drive front pushes that are accepted.
///
/// Reached through the capacity-1 `with_options` pass, so its failure carries the `built by
/// with_options` tag.
const FRONT_REFUSAL_SWAP_AT_CAP_ONE: u8 = 37;
/// `peek` memoises the residency it saw on its FIRST call and serves that latched run for the
/// rest of the cache's life, however much has been popped since (#180 part B, item 1).
///
/// It is **pure** by construction — two peeks with nothing changed in between agree, which is the
/// only repeatability law the kit had — and it is correct at the residency it latched, which is
/// the only residency any single cache instance was ever peeked at. Every sweep in the kit built
/// a **fresh** cache, popped it to the depth it wanted, and only then peeked, so the latch was
/// always armed by the very state it was about to be checked against. Only a cache that is
/// peeked, MUTATED, and peeked again separates it from a conforming one.
///
/// Takes the OWNED arm — a `VecDeque` behind `&self` cannot lend out entries it no longer holds,
/// and `Cache::peek`'s contract allows either arm ([`OWNED_PEEK`] is the witness that the kit
/// certifies it).
const MEMOISING_PEEK: u8 = 38;
/// `pop_back` hands back the **front** entry, but only once this cache has accepted a
/// `push_front` (#180 part B, item 2).
///
/// The natural shape is a ring whose `push_front` establishes the new head and leaves the tail
/// index naming a slot the prepend invalidated, so the tail pop reads the wrong end of the queue —
/// and only on a residency a prepend contributed to.
///
/// [`FIFO_POP_BACK`] is the same violation without the gate, and check 4 catches it at once. The
/// gate is what makes this cell a statement about the **front-built** residency alone: every
/// `pop_back` the kit drove ran against a cache built by `push_back` from empty — check 4's
/// drain, check 6's and check 8's back sweeps, and the alternating drain of #180 part B item 1
/// all fill with [`CacheHarness::filled`](super::cache) — while the one residency a `push_front`
/// builds was read by `pop_front` and never from the other end.
const TAIL_POP_AFTER_PUSH_FRONT: u8 = 39;
/// A `push_back` that lands while the cache already holds a front-pushed entry **permutes the
/// interior**: the two entries just behind the head swap places (#180 part B, item 3).
///
/// The natural shape is a ring whose `push_back` computes its slot from a head index a
/// `push_front` has since moved — right while the head has never moved, wrong once a prepend has
/// moved it, and wrong in the middle of the queue rather than at either end.
///
/// Every mixed residency the kit built was "one `push_back`, then N `push_front`s", so a
/// `push_back` **never once followed** a `push_front` anywhere in the kit: the seeding append is
/// the first operation, and the prepends all come after it. This cell is therefore inert in every
/// driver that existed — and it is gated on a fourth resident entry for the same reason
/// [`REORDERING_PUSH_FRONT`] is, so that index 2 is interior rather than the tail and `front`,
/// `back`, `len` and `remaining` all keep answering exactly as a conforming cache would.
const INTERLEAVED_PUSH_BACK_REORDERS: u8 = 40;
/// `peek` copies the whole residency when it fits in `W`, and walks **backward** from the cut
/// point when it does not — a truncating branch that is dead code at every window the kit drove
/// (#180 part B, item 4).
///
/// `peek` is generic over `W` and this cell reads `W::CAPACITY` off the type, which is the thing
/// the kit could not see: the window was `U4` in **every** driver, and with a fixture capacity of
/// 4 the guard `W::CAPACITY >= len` is true at every residency, so the branch behind it never
/// ran. Sweeping the prefill depth does not reach it either — that varies the buffer's REMAINING
/// capacity, and this is keyed on the type parameter, not on the room left.
///
/// Deliberately correct at `W = U1`: a one-entry run has no order, so reversing it is the
/// identity. Only a window strictly between 1 and the residency separates it from a conforming
/// cache, which is exactly the window nothing in the kit ever used.
const TRUNCATING_PEEK_WALKS_BACKWARD: u8 = 41;
/// `peek` has a single-slot fast path — `W::CAPACITY == 1` — and it grabs the **back** entry
/// (#180 part B, item 4).
///
/// [`Cache::peek_one`](crate::cache::Cache::peek_one)'s default body is literally `peek::<U1>`
/// into a one-slot buffer, so a one-slot special case is a shape the trait invites; this cell
/// overrides `peek_one` correctly (like every cell that is not [`WRONG_PEEK_ONE`], by not
/// overriding it at all) and gets `peek::<U1>` wrong, so the two cannot cover for each other. The
/// kit peeked through `U4` and nothing else, so the branch was unreachable.
///
/// Correct at `len <= 1`, where front and back are the same entry.
const SINGLE_SLOT_PEEK_TAKES_THE_BACK: u8 = 42;
/// `peek` walks the backing store from the head **without the modulo**, so once the live run
/// wraps past the end of the array it stops at the array end and serves only the part before it
/// (#180 part B, item 5).
///
/// The classic missing-`% capacity`, and the kit could not reach it: `filled` pushes back into an
/// empty cache, so the head index starts at 0 and every residency the kit builds keeps
/// `head + len <= capacity`. Popping the front advances the head and shortens the run by the same
/// step, so the sum is invariant; popping the back only shortens it. **Nothing wraps unless
/// something is pushed after something was popped**, and no driver did that until this issue.
///
/// Modelled with a synthetic [`head`](Queue::head) that moves exactly as a ring's does —
/// `pop_front` forward, `push_front` back, both modulo the capacity, `clear` to zero — read by
/// this cell and by [`WRAPPED_RUN_SPAN`] and by nothing else, so every other observable keeps
/// answering for the live run.
const WRAPPED_RUN_PEEK: u8 = 43;
/// [`WRAPPED_RUN_PEEK`]'s sibling on the combined span: `span()` ends at the last slot **before
/// the array end** once the live run wraps, rather than at the true back entry (#180 part B,
/// item 5).
///
/// The same missing modulo, in the other place a ring computes an end from `head + len`.
/// `front()`, `back()`, `back_span()` and `len()` all stay correct — they name a slot rather than
/// walking to one — so only the combined span diverges, and only at a residency that wraps.
/// [`STALE_SPAN_AFTER_POP_BACK`] is check 8's other span cell and is orthogonal: it is wrong
/// after a `pop_back` and right at every wrapped residency, since the rotation ends with a
/// `push_back` that refreshes what it reads.
const WRAPPED_RUN_SPAN: u8 = 44;
/// `peek` reads the buffer it was handed as a **prefix of its own run** whenever the spans say it
/// could be one, and starts one entry late — the "the caller already holds the parked token, do
/// not serve it twice" mistake (#180 part B, item 6).
///
/// The condition is a span comparison: if the last entry already in the buffer ends at or before
/// the front resident entry starts, this cache concludes the buffer is the head of the same
/// stream and skips its own front. And that condition is **exactly the one the kit could never
/// make true**. Every prefilled driver drew its prefill from
/// [`beyond_residency`](super::cache) — corpus tokens PAST the residency — so the buffer's spans
/// were always *greater* than every resident one, and the relation between the two was one fixed
/// value in every call the kit ever made.
///
/// It is also the inverse of the real one. `InputRef`'s peek fill pushes the parked token first
/// because "the parked token is the front of the stream, so it heads the window and the cache
/// fills in behind it" — so at the call site the buffer's entries **precede** the residency, and
/// that is the arrangement in which a cache can talk itself into deduplicating.
const PARKED_PREFIX_SKIPPING_PEEK: u8 = 45;
/// `try_pop_front_if` hands its predicate the **back** entry, and is otherwise conforming: the
/// return value and the residency are both computed from the FRONT, exactly as a correct
/// implementation would.
///
/// So an `Err` answer still removes nothing and hands the error straight back, an `Ok(())` answer
/// still removes exactly the front and returns it, and `pop_front_if` — the sibling arm — is left
/// alone entirely. The caller's validation predicate is simply run against a token the caller did
/// not ask about, which is how a parser ends up removing or retaining the front on the strength of
/// unrelated lookahead.
///
/// Check 9's Err and Ok closures used to be `|_| Err("no")` and `|_| Ok(())` — they discarded the
/// entry they were handed — so every assertion the check made about `try_pop_front_if` was about
/// its return value and its residency, both of which this cell gets right. The check *claimed*
/// "the predicate sees the front entry" for both methods and witnessed it for one.
///
/// Correct at `len <= 1`, where front and back are the same entry, so only the capacity-4 pass
/// separates it from a conforming cache.
const TRY_POP_FRONT_IF_PREDICATES_ON_THE_BACK: u8 = 46;
/// [`TRY_POP_FRONT_IF_PREDICATES_ON_THE_BACK`] on the other arm: `pop_front_if` hands its
/// predicate the **back** entry, and is otherwise conforming — a `true` answer still removes and
/// returns exactly the front, a `false` answer still removes nothing.
///
/// The assertion this cell is the witness for is not a new one: check 9 has recorded the span
/// `pop_front_if` hands its predicate since the check was written. What it never had was a cache
/// that disagrees with it — every fixture above predicates on `self.items.front()`, including
/// [`WRONG_POP_FRONT_IF`], whose defect is in what it returns and not in what it asks about. So
/// the `pop_front_if` half of "the predicate sees the front entry" was an assertion with nothing
/// behind it, which is the same shape as the `try_pop_front_if` half having no assertion at all,
/// and it turned up in the sweep that followed the review finding rather than in the finding.
///
/// Correct at `len <= 1`, where front and back are the same entry.
const POP_FRONT_IF_PREDICATES_ON_THE_BACK: u8 = 47;

// ── #183: the entry observable — cells that corrupt the token or the state and nothing else ──
//
// Every cell from here down leaves the SPANS of everything it touches exactly where a conforming
// cache would put them. That is the point: each is invisible to a kit that compares spans, which
// is what this kit did until #183, and each fires on exactly one of the two comparisons the entry
// observable adds. A generic fixture cannot fabricate an `L::State` value — but it can
// **re-associate** the ones it was handed, and that is the real defect class: a struct-of-arrays
// cache with index skew, an implementor pairing `state[i - 1]` with `token[i]`.
//
// The gates are the file's existing convention: each conforms at `len <= 1`, where the entry a
// neighbour skew reaches for is the entry itself.

/// The storage flagship: `push_back` stores the incoming span and token faithfully and stores the
/// **previously accepted entry's** `L::State` beside them (the first push into an empty cache
/// keeps its own, so there is nothing to lag behind).
///
/// So every span and every token in the cache is right, at every residency, and `back().state` —
/// the exact value `InputRef::resume` clones to rebuild a lexer with `with_state` + `bump` — is
/// always one entry behind. A ring that keeps its states in a parallel array and writes them at
/// the pre-increment index has this shape. It fires at the first two-resident residency check 3
/// builds, on the `back()` edge read, which is where the restore consequence is stated.
const STATE_LAG: u8 = 48;
/// `front()`/`back()` hand back references assembled out of **two** entries: the right span and
/// token, with the neighbour's `L::State` beside them. `front_span`/`back_span` are untouched.
///
/// [`WRONG_FRONT_IDENTITY`]'s successor one level deeper. That cell made the reference name the
/// wrong slot outright, which the span half of the edge-identity read catches; this one makes the
/// reference name the right slot and carry the wrong state, which nothing before #183 could see —
/// even the #180 fix compared only the reference's span.
const EDGE_STATE_SKEW: u8 = 49;
/// [`EDGE_STATE_SKEW`] on the other half: `front()`/`back()` return the right span and the right
/// state with the **neighbour's token** between them.
const EDGE_TOKEN_SKEW: u8 = 50;
/// `pop_front`/`pop_back` remove the right entry and return it with the `L::State` of the entry
/// that is now at that end. Honest when the pop emptied the cache, since there is no neighbour
/// left to borrow from.
///
/// The `pop_back` half is the input layer's rollback path, which drops an abandoned continuation
/// with a run of `pop_back` calls; a caller that resumes from one of those states resumes from a
/// position the lexer never occupied.
const POP_STATE_SKEW: u8 = 51;
/// [`POP_STATE_SKEW`] on the token half: the right entry with the remaining neighbour's token.
const POP_TOKEN_SKEW: u8 = 52;
/// [`OWNED_PEEK`] with the states re-associated: every `Maybe::Owned` clone it serves carries the
/// **front entry's** state rather than the one stored at its own position.
///
/// Position 0 is right by coincidence — the front is the first expected entry — exactly as
/// [`WRONG_OWNED_PEEK`]'s first entry is, and every entry behind it carries a state no cache ever
/// stored there. Spans and tokens are correct throughout, so the bound, the order and the
/// exact-sequence assertion all pass.
const OWNED_PEEK_STATE_SKEW: u8 = 53;
/// [`WRONG_OWNED_PEEK`] **minus the span corruption**: owned clones with the right spans and the
/// right states, and every token the front entry's.
///
/// This is the owned-arm instance of the same span-only comparison #183 is about. Its sibling is
/// caught today because cloning the whole front entry moves the spans too; take that away and the
/// kit had nothing left to notice.
const OWNED_PEEK_TOKEN_SKEW: u8 = 54;
/// `peek_one` answers with an owned entry carrying the front's span and token and the **back's**
/// state. Correct at `len <= 1`, where the two ends are the same entry.
///
/// `peek_one` names the entry the next `pop_front` will return, so a parser that peeks one and
/// resumes from what it was shown resumes from the wrong end of its own lookahead.
const PEEK_ONE_STATE_SKEW: u8 = 55;
/// [`PEEK_ONE_STATE_SKEW`] on the token half: the front's span and state with the back's token.
const PEEK_ONE_TOKEN_SKEW: u8 = 56;
/// A refused push hands back the offered span and token with a **resident** entry's `L::State`.
///
/// Span-identical to the token that was offered, so the refusal round-trip saw nothing wrong with
/// it before #183 — and the input layer's put-back path parks exactly this value and later
/// restores a lexer from it.
const REFUSAL_STATE_SWAP: u8 = 57;
/// `push_many`'s overflow iterator hands back entries with the right spans, in the right order,
/// carrying a **resident** entry's `L::State`.
///
/// The override lives in `push_many` alone, so `push_back` — and therefore check 3's refusal
/// round-trip — stays conforming and this cell reports at its own site.
const PUSH_MANY_STATE_SWAP: u8 = 58;
/// `try_pop_front_if` hands its predicate a reference carrying the front's span and token with
/// the **back's** `L::State`; the removal and the return value are the conforming ones, computed
/// from the front.
///
/// [`TRY_POP_FRONT_IF_PREDICATES_ON_THE_BACK`] one level deeper: that cell hands over the wrong
/// entry, which the span half catches; this one hands over the right entry with a state the
/// caller's validation predicate was never meant to see. A predicate that reads the state to
/// decide whether to consume is deciding on the wrong lexer position.
///
/// Scoped to `try_pop_front_if` as of #183's ninth round. It skewed both methods before, which
/// looked like twice the coverage and was not: `F: FnOnce` puts the defect on every arm, so the
/// first recording assertion to run reports it and the others are never reached — the
/// `pop_front_if` half could not fire while a `try_pop_front_if` arm ran earlier.
/// [`POP_FRONT_IF_PREDICATE_ARG_STATE_SKEW`] is the sibling that covers the other method.
const PREDICATE_ARG_STATE_SKEW: u8 = 59;
/// [`REFUSAL_STATE_SWAP`] on the token half: the offered span and state come back with a
/// **resident** entry's token between them.
///
/// Required by the #183 cross-model gate. The refusal round-trip compares the whole entry, and
/// its state sibling corrupts only the state — so without this cell the token half at that site
/// would be a comparison nothing could fail, which is the defect class the issue exists to close.
const REFUSAL_TOKEN_SWAP: u8 = 60;
/// [`PUSH_MANY_STATE_SWAP`] on the token half: overflow entries come back with the right spans
/// and the right states and a **resident** entry's token. Required by the #183 cross-model gate,
/// for the reason [`REFUSAL_TOKEN_SWAP`] is.
const PUSH_MANY_TOKEN_SWAP: u8 = 61;
/// [`PREDICATE_ARG_STATE_SKEW`] on the token half: `try_pop_front_if`'s predicate is handed the
/// front's span and state with the **back's** token. Required by the #183 cross-model gate, for
/// the reason [`REFUSAL_TOKEN_SWAP`] is, and scoped to the same method as its state sibling.
const PREDICATE_ARG_TOKEN_SKEW: u8 = 62;
/// `peek` caches a **lookahead frontier** — the `L::State` of the newest entry in the run it just
/// served — and every later pop on that instance reports that cached state instead of the state
/// of the entry it actually removed.
///
/// Caching the frontier is not the mistake; it is the exact value `InputRef::resume` wants out of
/// `back()`. Serving it from a **pop** is. The entry removed is the right one from the right end,
/// so the residency is correct at every step; the span returned is the removed entry's own, so
/// every span-level law is correct; only the `L::State` the caller resumes from belongs to a
/// different position.
///
/// Reachable at exactly one site, which is the point. Every other drain in the kit runs on an
/// instance that has never been peeked — `check_pop_order`, `check_push_front`'s three drains and
/// the residency sweeps in checks 6 and 8 all pop a cache built fresh and peek it, if at all,
/// only afterwards — so the frontier is unset and this cell is conforming there. Only
/// `check_peek_across_mutations` peeks an instance and then pops it, and until #183's review that
/// drain bound the popped value and checked it for **presence alone**: no mutant on any return
/// path could reach a value the kit discarded.
///
/// Honest at `len <= 1`, where the newest entry and the one a pop removes are the same one.
const POST_PEEK_POP_STATE_SKEW: u8 = 63;
/// Stores the offered entry **perfectly** and returns an `Ok` reference assembled from a
/// different resident: the pushed entry's span and token beside its neighbour's `L::State`.
///
/// [`Cache::push_back`](crate::cache::Cache::push_back) and
/// [`push_front`](crate::cache::Cache::push_front) promise `Ok` with *a reference to the cached
/// token*, so this violates the contract outright — and it is invisible to everything downstream,
/// because `front`, `back`, the pops and `peek` all read **storage**, and storage here is
/// flawless. The only observable that diverges is the value the push call itself handed back,
/// which every push in this kit reduced to `is_ok()` until #183's second review round.
///
/// Both push arms are corrupted, and both route through the kit's single accepted-push
/// comparison, so this one cell pins that read wherever it happens. Gated on a neighbour existing
/// after the push, so a push into an empty cache — the only push several checks ever make — is
/// conforming.
const WRONG_ACCEPTED_PUSH_REF: u8 = 64;
/// Refuses a `push_back` it had room to accept **and** corrupts the entry it hands back: the
/// offered span and token with a resident's `L::State` between them.
///
/// The compound is the point. A refusal with room to spare is already a violation the kit fails,
/// and the entry the refusal returns was the last `Cache` return value the kit received and did
/// not read — the `accepted_push_*` wrappers took `Err`, panicked on the refusal, and dropped the
/// value. So this cell cannot certify: any cache reaching it fails on the refusal regardless.
/// What it pins is that the kit **looks** before it panics, which is what makes the module docs'
/// shape enumeration true rather than nearly true.
///
/// Gated on a `push_front` having been accepted on this instance, which is what puts the first
/// hit at `interleaved`'s `push_back #1` — an `accepted_push_*` site — rather than at check 3's
/// or check 5's loops, where a refusal is a case the caller handles and compares in its own
/// wording. That separation is what keeps this cell off `REFUSAL_STATE_SWAP`'s and
/// `REFUSAL_TOKEN_SWAP`'s firing sites.
const UNEXPECTED_REFUSAL_ENTRY_SWAP: u8 = 65;
/// [`UNEXPECTED_REFUSAL_ENTRY_SWAP`] at the **mixed** `push_back` site: refuses with room to
/// spare from the moment the cache holds anything, and corrupts the entry it hands back.
///
/// Check 3's push is the site where the same call can produce a warranted refusal or an
/// unwarranted one, and until #183's fifth round the unwarranted branch asserted its way out
/// before the returned entry reached any comparison — the wrapper-level fix from round 4 did not
/// reach it, because this site never goes through `accepted_push_back`. Fires at check 3's second
/// push, the first `push_back` in the kit that lands on a non-empty cache with room left.
const EARLY_PUSH_BACK_REFUSAL_ENTRY_SWAP: u8 = 66;
/// The prepend twin: refuses a `push_front` with room to spare and corrupts what it hands back.
///
/// Fires in check 5's loop, at the first front push after the seeding append — the other mixed
/// site, and the other half of what round 5 closes.
const EARLY_PUSH_FRONT_REFUSAL_ENTRY_SWAP: u8 = 67;
/// [`PREDICATE_ARG_STATE_SKEW`] on the `pop_front_if` arm: the predicate is handed the front's
/// span and token with the **back's** `L::State`, and everything downstream — the removal, the
/// return value, the residency — is the conforming behaviour computed from the front.
///
/// From #183's ninth round, and it is the cell for the hole that round found. Check 9's
/// **declining** arm — a `false` predicate, which must remove nothing and answer `None` — used to
/// throw its predicate's argument away, because the site had been classified by its RETURN. The
/// return is genuinely an absence law; the argument is not, and `pop_front_if` hands caller code
/// a real entry before it learns the answer. So a cache could show the declining predicate
/// someone else's `L::State`, answer `None`, leave residency untouched, and be certified.
///
/// This is the mutant that makes the new comparison bite, and it lands **on the declining arm**:
/// it is the first recording predicate check 9 runs, so this cell reports there rather than at
/// the true-predicate arm below it. It fails on the entry comparison and not on residency —
/// nothing is removed — which is what distinguishes it from `WRONG_POP_FRONT_IF`.
const POP_FRONT_IF_PREDICATE_ARG_STATE_SKEW: u8 = 68;

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
  /// How many entries this cache will actually hold — what `push_back` refuses past.
  cap: usize,
  /// What `remaining()` reports against, which is the same as [`cap`](Self::cap) for every cell
  /// but [`WRONG_WITH_OPTIONS`], where `with_options` inflates it by one.
  claimed_cap: usize,
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
  /// The residency [`MEMOISING_PEEK`]'s first `peek` saw, latched and served for the rest of this
  /// cache's life. `RefCell` rather than `Cell` because it is read in place, and populated inside
  /// `peek`, which takes `&self`.
  memo: RefCell<Option<VecDeque<CachedTokenOf<'a, L>>>>,
  /// The lookahead frontier [`POST_PEEK_POP_STATE_SKEW`]'s `peek` caches — the newest served
  /// entry's `L::State` — and which every later pop on that instance then reports instead of the
  /// state of the entry it removed. `RefCell` for the same reason [`memo`](Self::memo) is: it is
  /// written inside `peek`, which takes `&self`, and `L::State` need not be `Copy`.
  ///
  /// Reset by `clear`, as a real cache's would be. Nothing but the pops read it, so every other
  /// observable of every other cell is untouched.
  frontier_state: RefCell<Option<L::State>>,
  /// The entry [`STATE_LAG`] was last **offered**, kept so that cell's accepted-push return can
  /// be honest while its storage is not.
  ///
  /// That is the shape the cell is about and the shape a struct-of-arrays cache actually has: the
  /// state array is written at the wrong index, and the reference handed back is assembled from
  /// the values the push was just given rather than read back out of the store. Keeping the two
  /// apart is also what keeps the cell pinning the `back()` edge read — where the restore
  /// consequence is stated — instead of stealing the accepted-push pin from
  /// [`WRONG_ACCEPTED_PUSH_REF`].
  last_offered: Option<CachedTokenOf<'a, L>>,
  /// A ring's head index, kept purely so that [`WRAPPED_RUN_PEEK`] and [`WRAPPED_RUN_SPAN`] can
  /// ask whether the live run has wrapped past the end of the backing array — `head + len > cap`.
  ///
  /// Moved exactly as a ring moves it: forward on `pop_front`, back on `push_front`, both modulo
  /// the capacity, and reset by `clear`. `push_back` and `pop_back` leave it alone. Nothing else
  /// reads it, so every observable of every other cell is untouched.
  head: usize,
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
    // Deliberately NOT `with_options(4)`: `WRONG_WITH_OPTIONS` is a defect in the second
    // constructor only, and it can only be one if the two do not share a body.
    Self::build(4, 4)
  }

  fn with_options(cap: usize) -> Self {
    if D == WRONG_WITH_OPTIONS {
      // Reports `cap` and holds one fewer. `new()` above is untouched, so this is invisible to
      // any driver that only ever calls `new()` — which is every driver there was.
      return Self::build(cap.saturating_sub(1), cap);
    }
    Self::build(cap, cap)
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
    self.claimed_cap - self.items.len()
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
      // A warranted refusal on the front arm that corrupts its own round-trip, at capacity 1 and
      // nowhere else. Not routed through `refuse`, which `push_back` shares: the whole point of
      // the cell is that the defect lives on THIS arm, at the one capacity check 5 never drove.
      if D == FRONT_REFUSAL_SWAP_AT_CAP_ONE
        && self.cap == 1
        && let Some(other) = self.items.pop_back()
      {
        self.items.push_back(tok);
        return Err(other);
      }
      return Err(self.refuse(tok));
    }
    if D == COLD_START_PUSH_FRONT && !self.warmed.get() {
      return Err(self.refuse(tok));
    }
    // Check 5's mixed site: a refusal with room, carrying a corrupted entry. Reached only after
    // the fullness check above, so warranted refusals stay honest and this cell cannot land on
    // the round-trip site `REFUSAL_STATE_SWAP` names.
    if D == EARLY_PUSH_FRONT_REFUSAL_ENTRY_SWAP
      && let Some(resident) = self.items.back()
    {
      return Err(Self::with_state(&tok, resident.state.clone()));
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
    // A ring's head steps back over the slot this prepend claims.
    if self.cap > 0 {
      self.head = (self.head + self.cap - 1) % self.cap;
    }
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
    // The prepend arm's twin of the accepted-push skew above.
    if D == WRONG_ACCEPTED_PUSH_REF && self.items.len() >= 2 {
      let own = self.items.front().expect("len >= 2");
      let neighbour = self.items.get(1).expect("len >= 2");
      return Ok(Self::ref_with_foreign_state(own, neighbour));
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
    // The same wrong end, reached only on a residency a `push_front` contributed to — a tail
    // index a prepend invalidated and nothing repaired. Every other `pop_back` the kit drives
    // runs against a cache filled by `push_back` from empty, where this is inert.
    if D == TAIL_POP_AFTER_PUSH_FRONT && self.front_pushes.get() > 0 {
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
    // #183: the restore path's own pop, handing back the right entry with the new back's state or
    // token beside it.
    self.skew_popped(popped, self.items.back())
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
    if D == POP_FRONT_IF_PREDICATES_ON_THE_BACK {
      // The predicate is handed the BACK entry; the removal and the return value below are the
      // conforming ones, taken from the front. Same shape as
      // `TRY_POP_FRONT_IF_PREDICATES_ON_THE_BACK`, on the arm whose recording assertion already
      // existed and had never been run against a cache that disagrees with it.
      if let Some(peeked) = self.items.back().map(CachedToken::as_ref)
        && predicate(peeked)
      {
        return self.pop_front_impl();
      }
      return None;
    }
    // #183: the predicate is handed the RIGHT entry with the back's state beside it. The removal
    // and the return value below are the conforming ones, taken from the front, so only what the
    // caller's validation predicate was shown diverges.
    //
    // `pop_front_if`'s own selector, separate from `try_pop_front_if`'s below. One selector used
    // to skew both methods, which read as covering both and could only ever report at one of
    // them: `F: FnOnce` makes the defect present on every arm, so the FIRST recording assertion
    // to run consumes it and no later one is reached. Split, each method's predicate-argument
    // read is pinned by a mutant that can actually land on it (#183 round 9).
    if D == POP_FRONT_IF_PREDICATE_ARG_STATE_SKEW && self.items.len() >= 2 {
      let answer = {
        let front = self.items.front().expect("len >= 2");
        let back = self.items.back().expect("len >= 2");
        predicate(Self::ref_with_foreign_state(front, back))
      };
      if answer {
        return self.pop_front_impl();
      }
      return None;
    }
    if let Some(peeked) = self.items.front().map(CachedToken::as_ref)
      && predicate(peeked)
    {
      return self.pop_front_impl();
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
    if D == TRY_POP_FRONT_IF_PREDICATES_ON_THE_BACK {
      // The predicate is handed the BACK entry; everything downstream of it is the conforming
      // body, computed from the front. Note that the outcome cannot gate this: `F: FnOnce` is
      // called exactly once, so which entry the predicate receives is settled before there is an
      // `Ok`/`Err` to branch on — this cell is therefore wrong on both paths at once.
      if let Some(peeked) = self.items.back().map(CachedToken::as_ref) {
        return match predicate(peeked) {
          Ok(()) => self.pop_front_impl().map(Ok),
          Err(e) => Some(Err(e)),
        };
      }
      return None;
    }
    // The same #183 skew on this arm, under `try_pop_front_if`'s OWN selectors — a caller's
    // validation predicate is run against the entry either method hands over, so each method
    // carries a mutant of its own rather than sharing one that only the earlier arm can consume.
    // `POP_FRONT_IF_PREDICATE_ARG_STATE_SKEW` is the sibling on the other method.
    if (D == PREDICATE_ARG_STATE_SKEW || D == PREDICATE_ARG_TOKEN_SKEW) && self.items.len() >= 2 {
      let answer = {
        let front = self.items.front().expect("len >= 2");
        let back = self.items.back().expect("len >= 2");
        let skewed = if D == PREDICATE_ARG_STATE_SKEW {
          Self::ref_with_foreign_state(front, back)
        } else {
          Self::ref_with_foreign_token(front, back)
        };
        predicate(skewed)
      };
      return match answer {
        Ok(()) => self.pop_front_impl().map(Ok),
        Err(e) => Some(Err(e)),
      };
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
        return None;
      }
      // #183: the overflow comes back at the right spans, in the right order, with a RESIDENT
      // entry's state (or token) substituted. The override is HERE and not in `push_back`, so
      // check 3's refusal round-trip stays conforming and this cell can only report at
      // `push_many`'s own site.
      if D == PUSH_MANY_STATE_SWAP || D == PUSH_MANY_TOKEN_SWAP {
        let over = refused?;
        let Some(resident) = self.items.back() else {
          return Some(over);
        };
        return Some(match D {
          PUSH_MANY_STATE_SWAP => Self::with_state(&over, resident.state.clone()),
          _ => Self::with_token(&over, resident.token.data.clone()),
        });
      }
      refused
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
    // A ring's `clear` puts the head back at slot zero, so a cleared cache does not start out
    // wrapped.
    self.head = 0;
    // A cleared cache has no lookahead frontier, so a real cache would drop it here too.
    *self.frontier_state.borrow_mut() = None;
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
    // Cached before any of the branches below, so it is set on every peek this instance answers
    // however it answers it. Caching the newest served entry's state is a reasonable thing for a
    // cache to do — it is the value `InputRef::resume` reads off `back()`; the defect is in
    // `skew_popped`, which reports it out of a POP.
    if D == POST_PEEK_POP_STATE_SKEW
      && let Some(back) = self.items.back()
    {
      *self.frontier_state.borrow_mut() = Some(back.state.clone());
    }
    // The window off the TYPE, not off the buffer. `buf.capacity()` is the same number, but the
    // two cells below are statements about a cache that branches on its `W` parameter, and this
    // is where that parameter is read.
    let w = <<W as crate::Window>::CAPACITY as Unsigned>::USIZE;
    let mut fill = buf.remaining_capacity().min(self.items.len());
    if D == SINGLE_SLOT_PEEK_TAKES_THE_BACK && w == 1 {
      // A one-slot fast path — the shape `peek_one`'s default composition asks for — reaching
      // for the wrong end. Dead code at every window but `U1`.
      if let Some(tok) = self.items.back()
        && fill > 0
      {
        buf.push_back(Maybe::Ref(tok.as_ref()));
      }
      return;
    }
    if D == TRUNCATING_PEEK_WALKS_BACKWARD && w < self.items.len() {
      // The branch taken only when the residency does NOT fit in the window: it walks backward
      // from the cut point instead of forward from the front. At `w >= len` the fast path below
      // runs and is correct, which is every call the kit made while its window was fixed at `U4`
      // and its fixture capacity was 4 — and at `w == 1` reversing a one-entry run is the
      // identity, so a single-slot window cannot see it either.
      for tok in self.items.iter().take(fill).rev() {
        buf.push_back(Maybe::Ref(tok.as_ref()));
      }
      return;
    }
    if D == WRAPPED_RUN_PEEK && self.wrapped() {
      // The walk from the head runs to the end of the array and stops there, because the index
      // that should have wrapped did not. Everything the kit built before this issue kept
      // `head + len <= cap`, so the truncation was always by zero.
      fill = fill.min(self.cap - self.head);
    }
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
    if D == DUPLICATING_PEEK && fill > 0 {
      // The right count, the right first entry, the right arm — and the SAME resident token
      // served `fill` times over rather than each resident token once. Guarded on `fill > 0` so
      // an empty cache or a full buffer still appends nothing, like a conforming `peek`.
      let repeated = self
        .items
        .front()
        .expect("fill > 0 implies at least one resident entry");
      for _ in 0..fill {
        buf.push_back(Maybe::Ref(repeated.as_ref()));
      }
      return;
    }
    if D == PARKED_PREFIX_SKIPPING_PEEK {
      // "The caller already holds the head of the stream." Read off the spans: if what is
      // already in the buffer ends at or before this cache's front begins, treat it as a prefix
      // of the same run and start one entry late. Against a buffer whose entries come AFTER the
      // residency — the only arrangement the kit ever built — the test is false and this is a
      // conforming `peek`.
      let already_ahead = match (buf.iter().next_back(), self.items.front()) {
        (Some(last), Some(front)) => last.span().end() <= front.token().span_ref().start(),
        _ => false,
      };
      for tok in self
        .items
        .iter()
        .skip(usize::from(already_ahead))
        .take(fill)
      {
        buf.push_back(Maybe::Ref(tok.as_ref()));
      }
      return;
    }
    if D == MEMOISING_PEEK {
      // The residency the FIRST peek saw, latched and served forever after. The bound is still
      // read off the buffer's remaining capacity, so the only thing that ever goes wrong is that
      // the run behind it stopped being the cache's own — which nothing can see until this
      // instance is mutated between two peeks.
      let mut memo = self.memo.borrow_mut();
      let run = memo.get_or_insert_with(|| self.items.clone());
      let latched = buf.remaining_capacity().min(run.len());
      for tok in run.iter().take(latched) {
        buf.push_back(Maybe::Owned(tok.clone()));
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
    if D == OWNED_PEEK_STATE_SKEW || D == OWNED_PEEK_TOKEN_SKEW {
      // #183 on the owned arm: the run, the order and the bound are `OWNED_PEEK`'s, and every
      // clone carries the FRONT entry's state (or token) instead of the one stored at its own
      // position. Position 0 is right by coincidence and everything behind it is a value no cache
      // ever stored there — with the spans left exactly where a conforming peek puts them, which
      // is the whole difference from `WRONG_OWNED_PEEK`.
      //
      // Guarded on `fill > 0` for the same reason that cell is: an empty cache or a full buffer
      // must still append nothing.
      if fill > 0 {
        let front = self
          .items
          .front()
          .expect("fill > 0 implies at least one resident entry")
          .clone();
        for tok in self.items.iter().take(fill) {
          let served = if D == OWNED_PEEK_STATE_SKEW {
            Self::with_state(tok, front.state.clone())
          } else {
            Self::with_token(tok, front.token.data.clone())
          };
          buf.push_back(Maybe::Owned(served));
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
    // #183: the FRONT entry — the right one — served through the owned arm with the BACK's state
    // (or token) beside it. Correct at `len <= 1`, where the two ends are the same entry, and
    // pure, since it is a function of the residency alone.
    if (D == PEEK_ONE_STATE_SKEW || D == PEEK_ONE_TOKEN_SKEW) && self.items.len() >= 2 {
      let front = self.items.front().expect("len >= 2");
      let back = self.items.back().expect("len >= 2");
      let served = if D == PEEK_ONE_STATE_SKEW {
        Self::with_state(front, back.state.clone())
      } else {
        Self::with_token(front, back.token.data.clone())
      };
      return Some(Maybe::Owned(served));
    }
    self.items.front().map(|tok| Maybe::Ref(tok.as_ref()))
  }

  fn front(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
    if D == WRONG_FRONT_IDENTITY {
      // The wrong END, while `front_span` below keeps naming the right one.
      return self.items.back().map(CachedToken::as_ref);
    }
    // #183: the right END, assembled with the neighbour's state or token. Gated on a neighbour
    // existing, so `len <= 1` is conforming — at one resident entry the front IS its own
    // neighbour and there is nothing to skew.
    if self.items.len() >= 2 {
      let own = self.items.front().expect("len >= 2");
      let neighbour = self.items.get(1).expect("len >= 2");
      if let Some(skewed) = Self::skewed_edge(own, neighbour) {
        return Some(skewed);
      }
    }
    self.items.front().map(CachedToken::as_ref)
  }

  fn back(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
    if D == WRONG_BACK_IDENTITY {
      return self.items.front().map(CachedToken::as_ref);
    }
    // [`front`](Self::front)'s #183 skew at the other end — and the end that matters most, since
    // `back()` is the entry `InputRef::resume` rebuilds a lexer from.
    let len = self.items.len();
    if len >= 2 {
      let own = self.items.back().expect("len >= 2");
      let neighbour = self.items.get(len - 2).expect("len >= 2");
      if let Some(skewed) = Self::skewed_edge(own, neighbour) {
        return Some(skewed);
      }
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
    if D == WRAPPED_RUN_SPAN && self.wrapped() {
      // The end computed by walking from the head without the modulo: it lands on the last slot
      // before the array end instead of on the true back. `back()` and `back_span()` name a slot
      // rather than walk to one, so they stay correct and only this diverges.
      let front = self.items.front()?;
      let last_before_the_end = self
        .items
        .get(self.cap - self.head - 1)
        .expect("a wrapped run is longer than the distance from the head to the array end");
      return Some(L::Span::new(
        front.token().span_ref().start(),
        last_before_the_end.token().span_ref().end(),
      ));
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
  /// A fresh queue that holds `cap` entries and reports `claimed_cap` free slots while empty.
  ///
  /// The two are the same number for every cell but [`WRONG_WITH_OPTIONS`]. Inherent rather than
  /// shared through `new`/`with_options` so that those two can differ, which is what it takes for
  /// a defect to live in the second constructor alone.
  fn build(cap: usize, claimed_cap: usize) -> Self {
    Self {
      items: VecDeque::with_capacity(cap),
      graveyard: VecDeque::new(),
      cap,
      claimed_cap,
      peeks: Cell::new(0),
      prefilled: Cell::new(false),
      warmed: Cell::new(false),
      front_pushes: Cell::new(0),
      last_pushed_back_span: None,
      poisoned: false,
      stashed: None,
      peek_one_at_len: Cell::new(None),
      memo: RefCell::new(None),
      frontier_state: RefCell::new(None),
      last_offered: None,
      head: 0,
    }
  }

  /// Whether the live run has wrapped past the end of the backing array — the state a ring reaches
  /// only by pushing after popping, and the one the two wrap cells are keyed on.
  fn wrapped(&self) -> bool {
    self.head + self.items.len() > self.cap
  }

  // ── #183: the four re-associations every entry cell below is built out of ──────────
  //
  // A fixture generic over `L` cannot invent an `L::State` or an `L::Token`; what it can do is
  // pair one entry's span with another entry's state or token, which is exactly the defect a
  // struct-of-arrays cache with index skew has. These four constructors are the whole of the
  // machinery, so every cell below is a gate and one call.

  /// A clone of `entry` carrying `state` instead of its own — the span and the token untouched.
  fn with_state(entry: &CachedTokenOf<'a, L>, state: L::State) -> CachedTokenOf<'a, L> {
    let mut out = entry.clone();
    out.state = state;
    out
  }

  /// A clone of `entry` carrying `token` instead of its own — the span and the state untouched.
  fn with_token(entry: &CachedTokenOf<'a, L>, token: L::Token) -> CachedTokenOf<'a, L> {
    let mut out = entry.clone();
    out.token.data = token;
    out
  }

  /// A reference-shaped entry assembled out of two resident ones: `own`'s span and token beside
  /// `state_from`'s state. What a cache that reads its edges out of parallel arrays, one index
  /// apart, hands a caller.
  fn ref_with_foreign_state<'s>(
    own: &'s CachedTokenOf<'a, L>,
    state_from: &'s CachedTokenOf<'a, L>,
  ) -> CachedTokenRefOf<'s, 'a, L> {
    CachedToken::new(
      Spanned::new(&own.token.span, &own.token.data),
      &state_from.state,
    )
  }

  /// [`ref_with_foreign_state`](Self::ref_with_foreign_state) on the other half: `own`'s span and
  /// state beside `token_from`'s token.
  fn ref_with_foreign_token<'s>(
    own: &'s CachedTokenOf<'a, L>,
    token_from: &'s CachedTokenOf<'a, L>,
  ) -> CachedTokenRefOf<'s, 'a, L> {
    CachedToken::new(
      Spanned::new(&own.token.span, &token_from.token.data),
      &own.state,
    )
  }

  /// The two edge cells' shared body: the entry at one end of the queue, handed back with its
  /// neighbour's state ([`EDGE_STATE_SKEW`]) or its neighbour's token ([`EDGE_TOKEN_SKEW`]).
  /// `None` when neither cell is selected or the queue is too short for a neighbour to exist, in
  /// which case the caller answers conformingly.
  fn skewed_edge<'s>(
    own: &'s CachedTokenOf<'a, L>,
    neighbour: &'s CachedTokenOf<'a, L>,
  ) -> Option<CachedTokenRefOf<'s, 'a, L>> {
    match D {
      EDGE_STATE_SKEW => Some(Self::ref_with_foreign_state(own, neighbour)),
      EDGE_TOKEN_SKEW => Some(Self::ref_with_foreign_token(own, neighbour)),
      _ => None,
    }
  }

  /// The two pop cells' shared body: the right entry, carrying the state ([`POP_STATE_SKEW`]) or
  /// the token ([`POP_TOKEN_SKEW`]) of whatever is left at that end of the queue.
  ///
  /// Honest when the pop emptied the cache — there is no neighbour to borrow from, and a cell
  /// that answered wrongly there would be caught by a residency the entry comparison does not
  /// need to reach.
  fn skew_popped(
    &self,
    popped: Option<CachedTokenOf<'a, L>>,
    neighbour: Option<&CachedTokenOf<'a, L>>,
  ) -> Option<CachedTokenOf<'a, L>> {
    let tok = popped?;
    // [`POST_PEEK_POP_STATE_SKEW`]: the state an earlier `peek` on THIS instance cached, reported
    // by every pop after it. Gated on a peek having happened rather than on the residency — the
    // frontier is what makes it wrong, and there is no frontier until something peeked. Cloned
    // out of the `RefCell` before the `if let` so no borrow guard outlives the scrutinee.
    if D == POST_PEEK_POP_STATE_SKEW {
      let frontier = self.frontier_state.borrow().clone();
      return match frontier {
        Some(frontier) => Some(Self::with_state(&tok, frontier)),
        None => Some(tok),
      };
    }
    let Some(neighbour) = neighbour else {
      return Some(tok);
    };
    match D {
      POP_STATE_SKEW => Some(Self::with_state(&tok, neighbour.state.clone())),
      POP_TOKEN_SKEW => Some(Self::with_token(&tok, neighbour.token.data.clone())),
      _ => Some(tok),
    }
  }

  /// The refusal round-trip — or, under `SWAPPING_REFUSAL`, its violation: the caller is handed a
  /// resident entry instead of the token it offered, which silently swallows that token.
  fn refuse(&mut self, tok: CachedTokenOf<'a, L>) -> CachedTokenOf<'a, L> {
    if D == SWAPPING_REFUSAL
      && let Some(other) = self.items.pop_back()
    {
      self.items.push_back(tok);
      return other;
    }
    // #183: the offered token comes back at its own span, with a RESIDENT entry's state or token
    // substituted. Span-identical to what was offered, so the round-trip saw nothing wrong with
    // it while it compared spans — and the input layer parks this value and later restores from
    // it.
    if (D == REFUSAL_STATE_SWAP || D == REFUSAL_TOKEN_SWAP)
      && let Some(resident) = self.items.back()
    {
      return match D {
        REFUSAL_STATE_SWAP => Self::with_state(&tok, resident.state.clone()),
        _ => Self::with_token(&tok, resident.token.data.clone()),
      };
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
    // #183 round 4: a refusal the contract does not allow — there is room — reached only once a
    // prepend has landed on this instance, which is the gate that puts the first hit on an
    // `accepted_push_*` site rather than on the two loops that drive refusals deliberately.
    //
    // The corruption is applied HERE and not in `refuse`, which the capacity check above shares:
    // routing it through there would corrupt the WARRANTED refusals too, and this cell would
    // land on check 3's round-trip — `REFUSAL_STATE_SWAP`'s site — instead of on the unexpected
    // one it exists for. (It did, the first time this cell was written.)
    if D == UNEXPECTED_REFUSAL_ENTRY_SWAP && self.front_pushes.get() > 0 {
      return Err(match self.items.back() {
        Some(resident) => Self::with_state(&tok, resident.state.clone()),
        None => tok,
      });
    }
    // The same shape at check 3's MIXED site: refuse with room from the first non-empty cache,
    // and corrupt the entry handed back. Reached before any `accepted_push_back`, so it lands on
    // the branch round 4's wrapper-level fix could not see.
    if D == EARLY_PUSH_BACK_REFUSAL_ENTRY_SWAP
      && let Some(resident) = self.items.back()
    {
      return Err(Self::with_state(&tok, resident.state.clone()));
    }
    self.warmed.set(true);
    // Read before the move below, for `STALE_SPAN_AFTER_POP_BACK`'s `span()` override — see
    // that constant's doc for why this is never cleared or refreshed by a pop.
    self.last_pushed_back_span = Some((*tok.token().span_ref()).clone());
    // #183's storage flagship: the span and the token go in faithfully, and the state that goes
    // in beside them is the previously accepted entry's. The first push into an empty cache has
    // no predecessor and keeps its own, which is why check 3's first residency is conforming and
    // the second is not.
    //
    // The `Ok` reference this arm returns is built from the entry it was OFFERED, not read back
    // out of the store — see `last_offered`. That keeps the cell's single divergence in storage,
    // where its doc says it is, so it pins the `back()` edge read rather than the accepted-push
    // read that `WRONG_ACCEPTED_PUSH_REF` exists for.
    if D == STATE_LAG {
      let lagged = match self.items.back() {
        Some(prev) => Self::with_state(&tok, prev.state.clone()),
        None => tok.clone(),
      };
      self.last_offered = Some(tok);
      self.items.push_back(lagged);
      return Ok(self.last_offered.as_ref().expect("just set").as_ref());
    }
    self.items.push_back(tok);
    if D == INTERLEAVED_PUSH_BACK_REORDERS && self.front_pushes.get() > 0 && self.items.len() > 3 {
      // A slot computed from a head index a `push_front` has since moved. Interior only — index 2
      // is the tail while there are three resident, so the swap waits for a fourth, and neither
      // the head nor the tail this push just established ever moves. Gated on a prepend having
      // happened at all, which is what no driver in the kit ever put in front of a `push_back`.
      self.items.swap(1, 2);
    }
    // #183 round 3: storage above is faithful; the reference handed back is not. Assembled from
    // the entry just pushed and its neighbour's state, so nothing that reads the store can see it.
    if D == WRONG_ACCEPTED_PUSH_REF && self.items.len() >= 2 {
      let n = self.items.len();
      let own = self.items.get(n - 1).expect("len >= 2");
      let neighbour = self.items.get(n - 2).expect("len >= 2");
      return Ok(Self::ref_with_foreign_state(own, neighbour));
    }
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
    if D == RESURRECTING_CLEAR
      && self.items.is_empty()
      && let Some(tok) = self.stashed.take()
    {
      return Some(tok);
    }
    let popped = self.items.pop_front();
    // A ring's head steps forward over the slot this pop vacates.
    if popped.is_some() && self.cap > 0 {
      self.head = (self.head + 1) % self.cap;
    }
    if D == STALE_RESIDENCY_PEEK {
      // The entry leaves the live run and stays in the store — a ring's head moving past a slot
      // it does not clear. Nothing but `peek` reads it, so `len`, `front`, `back` and `remaining`
      // all keep answering for the live run alone.
      if let Some(tok) = popped.as_ref() {
        self.graveyard.push_back(tok.clone());
      }
    }
    // #183: the right entry with the new front's state or token beside it, under the two pop
    // cells and nowhere else.
    self.skew_popped(popped, self.items.front())
  }
}

/// Runs the kit over `Queue<D>`, so each cell below is one line.
///
/// Two passes: `Queue::new()` at capacity 4, then `Queue::with_options(1)` at capacity 1. The
/// second is what drives `Cache::with_options` at all (#180 part A, item 7) — the kit cannot
/// reach it on its own, since `Options` is an associated type it has no way to fabricate a value
/// of — and capacity 1 is chosen for it because that is the most degenerate capacity a cache can
/// have and still hold anything: several checks take a different path there, and one of them,
/// the prepend law, could not be driven at all before #180 part A, item 10.
fn run_queue<const D: u8>() {
  CacheHarness::<CLex<'_>, Queue<'_, CLex<'_>, D>>::new(SRC)
    .named("third-party queue")
    .also_built_by(|| <Queue<'_, CLex<'_>, D> as Cache<'_, CLex<'_>, ()>>::with_options(1))
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

/// #180 part A, item 9: check 6's "each resident token once" clause had no fixture of its own —
/// it was implied by the exact-sequence assertion and never witnessed. This is the witness. No
/// new assertion goes with it: see [`DUPLICATING_PEEK`] and `check_peek_at` for why a
/// distinctness check of its own provably could not be made to fail.
#[test]
#[should_panic(expected = "OLDEST FIRST")]
fn cache_kit_catches_a_peek_that_serves_one_token_repeatedly() {
  run_queue::<DUPLICATING_PEEK>();
}

/// #180 part A, item 7: the kit built every cache it tested with `C::new()`, so `with_options`
/// was never called and a conformant `new` covered for an arbitrarily broken second constructor.
///
/// The `expected` string is the point: it is the `built by with_options` tag the second pass puts
/// in every message, so this test fails if the defect is ever caught on the `C::new()` pass
/// instead — which would mean the fixture, not the kit, had changed.
#[test]
#[should_panic(expected = "built by with_options")]
fn cache_kit_catches_a_wrong_with_options() {
  run_queue::<WRONG_WITH_OPTIONS>();
}

/// #180 part A, item 10: check 5 returned at its first line for `cap < 2`, so capacity 1 — the
/// one capacity where EVERY front push is refused — was never driven at all. The prepend order
/// is genuinely not observable there, but the refusal half is, and no other check reaches it on
/// the `push_front` arm. Caught by check 5's round-trip, on the capacity-1 pass.
#[test]
#[should_panic(expected = "a refused push_front returned a DIFFERENT token")]
fn cache_kit_catches_a_corrupt_front_refusal_at_capacity_one() {
  run_queue::<FRONT_REFUSAL_SWAP_AT_CAP_ONE>();
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

/// The two `try_pop_front_if` closures used to be `|_| Err("no")` and `|_| Ok(())`: they threw
/// away the entry they were handed, so the check asserted the return value and the residency and
/// nothing about WHICH entry the caller's validation predicate was run against — while its own
/// prose, and the module docs' check 9, claimed the predicate sees the front for both methods.
/// The `pop_front_if` arm did record it; this arm did not, and an assertion that names a property
/// it does not test reads as coverage.
///
/// The expected message names the **Err** path, which is the one that runs first. A cache cannot
/// key the entry it hands over on an answer it has not received yet — `F: FnOnce`, called once —
/// so a wrong-entry defect is wrong on both paths, and the first recording assertion to run is the
/// one that reports it. Both are asserted regardless, so that neither path depends on the other
/// running first.
#[test]
#[should_panic(expected = "try_pop_front_if()'s Err-path predicate was handed")]
fn cache_kit_catches_a_try_pop_front_if_that_predicates_on_the_back() {
  run_queue::<TRY_POP_FRONT_IF_PREDICATES_ON_THE_BACK>();
}

/// The same defect on the `pop_front_if` arm, and the witness for an assertion that was already
/// there.
///
/// Check 9 has recorded the span `pop_front_if` hands its predicate since it was written, but
/// every fixture predicated on the front, so nothing ever made that assertion fire — the
/// `try_pop_front_if` finding is an assertion missing where the property is claimed, and this is
/// the same property claimed by an assertion with no mutant behind it. Both halves of check 9's
/// "the predicate sees the front entry" are witnessed as of this pair.
///
/// The `expected` string names the **declining** arm as of #183's ninth round, and this is the
/// one firing site in the kit that round moved. It is the same assertion on the same method —
/// `assert_recorded_predicate_arg`'s span half, reached through `recording_pop_front_if` — at an
/// earlier call: the false-predicate arm did not read its argument at all before that round, so
/// the true-predicate arm below was the first `pop_front_if` that looked. Closing the hole put a
/// reader in front of it. The move was found by the before/after firing-message diff rather than
/// reasoned about, and the vacated arm still carries the identical routed assertion.
#[test]
#[should_panic(expected = "pop_front_if()'s false predicate was handed")]
fn cache_kit_catches_a_pop_front_if_that_predicates_on_the_back() {
  run_queue::<POP_FRONT_IF_PREDICATES_ON_THE_BACK>();
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

/// #180 part B, item 6: every prefill the kit built came from PAST the residency, so the buffer's
/// spans were always the greater ones and the relation between the two was one fixed value in
/// every call `peek` ever received here — and the inverse of the one the real call site produces,
/// where the parked token heads the window and the cache fills in behind it.
///
/// The expected message names the reversed arrangement at its shallowest depth. This fixture is
/// conforming in the original arrangement — its span test is simply false there — so this test
/// failing means the kit is back to prefilling only with tokens that come after the residency,
/// where a `peek` that decides the caller already holds its own front cannot act on it.
#[test]
#[should_panic(expected = "bounded-peek/prefilled-with-earlier-tokens at depth 1")]
fn cache_kit_catches_a_peek_that_skips_a_prefix_it_thinks_the_caller_holds() {
  run_queue::<PARKED_PREFIX_SKIPPING_PEEK>();
}

/// #180 part B, item 5: the ring never wrapped. `filled` pushes back into an empty cache, so the
/// head starts at slot zero, and a residency reached by popping keeps `head + len <= capacity` —
/// popping the front advances the head and shortens the run by the same step. A run wraps only
/// when something is **pushed after something was popped**, and no driver did that.
///
/// The expected message names the shallowest wrapped residency there is: one pop off the front
/// and one push back on. So this test failing means the rotation is gone and a missing
/// `% capacity` in `peek`'s walk from the head is back to truncating by zero everywhere.
#[test]
#[should_panic(expected = "a ring's live run starts at slot 1 and runs past the end of its array")]
fn cache_kit_catches_a_peek_that_stops_at_the_end_of_a_wrapped_ring() {
  run_queue::<WRAPPED_RUN_PEEK>();
}

/// The same missing modulo in the other place a ring computes an end from `head + len`: the
/// combined span.
///
/// `back()`, `back_span()` and `len()` name a slot rather than walk to one, so they stay correct
/// and only `span()` diverges — which means `check_peek_at`'s oracle cannot see this and the
/// wrapped residency has to be handed to check 8's as well. This test failing means it no longer
/// is.
#[test]
#[should_panic(expected = "combined-span] against a WRAPPED residency")]
fn cache_kit_catches_a_span_that_stops_at_the_end_of_a_wrapped_ring() {
  run_queue::<WRAPPED_RUN_SPAN>();
}

/// #180 part B, item 4: `W` was `U4` in every driver the kit had, so a `peek` that reads
/// `W::CAPACITY` off its own type parameter and branches on it was invisible.
///
/// The expected message names **window 3** — the only window the kit drives that is narrower than
/// the residency and wider than one entry, which is the only shape in which the "does not fit"
/// branch both runs and has an order to get wrong. So this test failing means the window sweep
/// lost its middle value and `peek` is back to only ever being asked for a run that fits whole.
#[test]
#[should_panic(expected = "bounded-peek/window 3")]
fn cache_kit_catches_a_peek_whose_truncating_branch_walks_backward() {
  run_queue::<TRUNCATING_PEEK_WALKS_BACKWARD>();
}

/// The other end of the same sweep: the **single-slot** window.
///
/// The expected message names window 1. `peek_one`'s default body is `peek::<U1>`, so a one-slot
/// fast path is a specialization the trait invites — and this fixture leaves `peek_one` itself
/// correct, so only a driver that peeks through a one-slot window can see it. This test failing
/// means that window is gone.
#[test]
#[should_panic(expected = "bounded-peek/window 1")]
fn cache_kit_catches_a_single_slot_peek_that_takes_the_back() {
  run_queue::<SINGLE_SLOT_PEEK_TAKES_THE_BACK>();
}

/// #180 part B, item 3: every mixed residency the kit built was "one `push_back`, then N
/// `push_front`s", so a `push_back` never once followed a `push_front` anywhere in the kit.
///
/// The expected message names **length 4**: the fixture leaves the queue alone until there are
/// four resident, since below that the pair it swaps includes the tail. So this test failing means
/// the alternating build is gone, or its length is pinned short of the capacity — either way a
/// `push_back` is back to only ever meeting a queue no prepend has touched.
#[test]
#[should_panic(expected = "interleaved-order at length 4")]
fn cache_kit_catches_an_append_that_reorders_after_a_prepend() {
  run_queue::<INTERLEAVED_PUSH_BACK_REORDERS>();
}

/// #180 part B, item 2: the front-built residency was drained with `pop_front` and never from the
/// other end, and every `pop_back` the kit did drive ran against a cache `push_back` built from
/// empty.
///
/// The expected message names the shallowest front-built residency there is — one `push_back` and
/// one `push_front`. `TAIL_POP_AFTER_PUSH_FRONT` is conforming at every back-built residency the
/// kit sweeps, so this test failing means the mirrored drain is gone and the two dimensions —
/// which end built the queue, and which end reads it — are back to being tied together.
#[test]
#[should_panic(expected = "full-order-from-the-back at depth 1")]
fn cache_kit_catches_a_tail_pop_broken_by_a_prepend() {
  run_queue::<TAIL_POP_AFTER_PUSH_FRONT>();
}

/// #180 part B, item 1: no cache instance was ever peeked, MUTATED, and peeked again — every
/// sweep built a fresh cache, popped it to the residency it wanted, and only then peeked.
///
/// The expected message names the first state reached by mutating an instance the kit had already
/// peeked. `MEMOISING_PEEK` is conforming at the residency it latched and pure at every one of
/// them, so this test failing means the kit went back to a fresh instance per residency, where a
/// latch is always armed by the very state it is about to be checked against.
#[test]
#[should_panic(expected = "ALREADY peeked, after 1 pop(s)")]
fn cache_kit_catches_a_memoising_peek() {
  run_queue::<MEMOISING_PEEK>();
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

// ── #183: the entry observable, and the fifteen cells that keep it from being vacuous ────
//
// Every cell below leaves the spans of everything it touches where a conforming cache would put
// them, so **every one of them passed the kit in full before #183**. Each pins exactly one of the
// two comparisons the entry observable adds, at one site, through the `entry-token` and
// `entry-state` message tags: an `expected` string that named only the site would match the span
// half too and prove nothing.

/// The corpus is the substrate every entry cell below stands on, and it is silent when it breaks:
/// a `SRC` edit that reintroduced a repeated token payload, or a lexer change that stopped the
/// state advancing, would leave every mutant here *passing* — the kit would compare, and the
/// comparison would be vacuous, which is the exact defect class #183 exists to close, one level
/// down.
///
/// So the distinctness is asserted directly, off the lexer rather than through the kit: twelve
/// items, pairwise distinct spans, tokens and states, with the kinds alternating so that a
/// neighbour skew is visible on the kind and not only on the payload.
#[test]
fn corpus_is_pairwise_distinct_on_all_three_axes() {
  let mut lexer = <CLex<'_> as Lexer<'_>>::new(SRC);
  let mut spans = Vec::new();
  let mut tokens = Vec::new();
  let mut states = Vec::new();
  while spans.len() < 12 {
    let Some(res) = lexer.lex() else { break };
    let span = lexer.span();
    let state = *lexer.state();
    let tok = res.expect("the corpus source lexes cleanly");
    spans.push(span);
    tokens.push(tok);
    states.push(state);
  }
  assert_eq!(
    spans.len(),
    12,
    "SRC must lex to the twelve items the widest capacity (8) plus a full 4-slot peek window \
     needs; it lexed {} — every `beyond_residency`/`filled_from` demand in the kit is bounded by \
     capacity + window, so a shorter SRC turns cell failures into kit-usage panics",
    spans.len()
  );
  for i in 0..spans.len() {
    assert_eq!(
      tokens[i].kind(),
      if i % 2 == 0 { CKind::Word } else { CKind::Num },
      "SRC's token kinds must alternate, so a cache that swaps two neighbours is visible on the \
       kind and not only on the payload; position {i} is {:?}",
      tokens[i].kind()
    );
    assert_eq!(
      states[i].lexed,
      u32::try_from(i).expect("twelve fits") + 1,
      "the corpus state must advance once per token, so the state cached beside each entry is \
       that entry's own"
    );
    for j in (i + 1)..spans.len() {
      assert_ne!(
        spans[i], spans[j],
        "corpus spans must be pairwise distinct: positions {i} and {j} share one"
      );
      assert_ne!(
        tokens[i], tokens[j],
        "corpus tokens must be pairwise distinct, or the kit's token comparison cannot \
         discriminate a swap between positions {i} and {j}"
      );
      assert_ne!(
        states[i], states[j],
        "corpus states must be pairwise distinct, or the kit's state comparison cannot \
         discriminate a re-association between positions {i} and {j}"
      );
    }
  }
}

/// Mutant 1, the storage flagship: `push_back` stores every span and every token faithfully and
/// stores the **previous** entry's `L::State` beside them.
///
/// The expected message names the `back()` edge read, which is the entry `InputRef::resume`
/// rebuilds a lexer from and the one whose state message carries the restore consequence. It
/// fires at the first two-resident residency check 3 builds — the shallowest residency at which a
/// lag is a lag at all.
#[test]
#[should_panic(expected = "entry-state] edge-identity back()")]
fn cache_kit_catches_a_push_back_that_stores_the_previous_entrys_state() {
  run_queue::<STATE_LAG>();
}

/// Mutant 2: `front()`/`back()` name the right slot and carry the neighbour's state.
///
/// The expected message names the `front()` read, which is the first edge the residency oracle
/// compares. `WRONG_FRONT_IDENTITY` is the same accessor caught by the span half; this cell is
/// what is left of that defect once the span is right, and nothing before #183 could see it.
#[test]
#[should_panic(expected = "entry-state] edge-identity front()")]
fn cache_kit_catches_an_edge_reference_carrying_the_neighbours_state() {
  run_queue::<EDGE_STATE_SKEW>();
}

/// Mutant 3: [`EDGE_STATE_SKEW`] on the token half.
#[test]
#[should_panic(expected = "entry-token] edge-identity front()")]
fn cache_kit_catches_an_edge_reference_carrying_the_neighbours_token() {
  run_queue::<EDGE_TOKEN_SKEW>();
}

/// Mutant 4: `pop_front`/`pop_back` return the right entry with the remaining neighbour's state.
///
/// The expected message names check 4's first `pop_front`, which is the first pop in the whole
/// kit that leaves a neighbour behind — the pops in checks 2 and 5's empty-cache drivers empty
/// the cache, and this cell is honest there by construction.
#[test]
#[should_panic(expected = "entry-state] order pop_front #0")]
fn cache_kit_catches_a_pop_that_returns_the_neighbours_state() {
  run_queue::<POP_STATE_SKEW>();
}

/// Mutant 5: [`POP_STATE_SKEW`] on the token half.
#[test]
#[should_panic(expected = "entry-token] order pop_front #0")]
fn cache_kit_catches_a_pop_that_returns_the_neighbours_token() {
  run_queue::<POP_TOKEN_SKEW>();
}

/// Mutant 6: the owned peek arm serves the right run with every state replaced by the front's.
///
/// The expected message names the empty-buffer peek at the full cache — `bounded-peek against`,
/// which is the run comparison and not `peek_one`'s or the prefilled sweep's. Position 0 is right
/// by coincidence, exactly as `WRONG_OWNED_PEEK`'s is, so it takes a residency of two before the
/// kit is asked anything at all.
#[test]
#[should_panic(expected = "entry-state] bounded-peek against")]
fn cache_kit_catches_an_owned_peek_that_serves_one_state_for_every_entry() {
  run_queue::<OWNED_PEEK_STATE_SKEW>();
}

/// Mutant 7: `WRONG_OWNED_PEEK` **minus the span corruption** — the owned-arm instance of the
/// span-only comparison #183 is about.
#[test]
#[should_panic(expected = "entry-token] bounded-peek against")]
fn cache_kit_catches_an_owned_peek_that_serves_one_token_for_every_entry() {
  run_queue::<OWNED_PEEK_TOKEN_SKEW>();
}

/// Mutant 8: `peek_one` names the front and carries the back's state.
///
/// The expected message names `peek_one()` specifically, so this cell cannot be satisfied by the
/// run comparison above it — `peek` is untouched here, and a cell whose `expected` said only
/// `bounded-peek` would not have said which of the two it pinned.
#[test]
#[should_panic(expected = "entry-state] bounded-peek peek_one()")]
fn cache_kit_catches_a_peek_one_that_carries_the_backs_state() {
  run_queue::<PEEK_ONE_STATE_SKEW>();
}

/// Mutant 9: [`PEEK_ONE_STATE_SKEW`] on the token half.
#[test]
#[should_panic(expected = "entry-token] bounded-peek peek_one()")]
fn cache_kit_catches_a_peek_one_that_carries_the_backs_token() {
  run_queue::<PEEK_ONE_TOKEN_SKEW>();
}

/// Mutant 10: a refused push hands the offered token back at its own span with a resident's
/// state.
///
/// `SWAPPING_REFUSAL` is the same site caught by the span half — it hands back a whole different
/// entry. This is what survives when the span is right, and it is the value the input layer's
/// put-back parks and later restores a lexer from.
#[test]
#[should_panic(expected = "entry-state] refusal-round-trip push_back")]
fn cache_kit_catches_a_refusal_that_swaps_in_a_residents_state() {
  run_queue::<REFUSAL_STATE_SWAP>();
}

/// Mutant 13 (added by the #183 cross-model gate): the refusal round-trip's **token** half.
///
/// Without this cell that half would be a comparison nothing in the suite could fail: mutant 10
/// corrupts only the state, and no other cell reaches a refused push's return value at all.
#[test]
#[should_panic(expected = "entry-token] refusal-round-trip push_back")]
fn cache_kit_catches_a_refusal_that_swaps_in_a_residents_token() {
  run_queue::<REFUSAL_TOKEN_SWAP>();
}

/// Mutant 11: `push_many`'s overflow comes back at the right spans, in order, with a resident's
/// state.
///
/// The override is in `push_many` alone, so check 3's refusal round-trip stays conforming and the
/// cell reports at its own site — which is what makes the `expected` string a pin on **this**
/// comparison rather than on the refusal one.
#[test]
#[should_panic(expected = "entry-state] push-many overflow entry #0")]
fn cache_kit_catches_a_push_many_that_rewrites_the_overflows_state() {
  run_queue::<PUSH_MANY_STATE_SWAP>();
}

/// Mutant 14 (added by the #183 cross-model gate): the `push_many` overflow's **token** half, for
/// the reason [`cache_kit_catches_a_refusal_that_swaps_in_a_residents_token`] exists.
#[test]
#[should_panic(expected = "entry-token] push-many overflow entry #0")]
fn cache_kit_catches_a_push_many_that_rewrites_the_overflows_token() {
  run_queue::<PUSH_MANY_TOKEN_SWAP>();
}

/// Mutant 12: `try_pop_front_if` hands its predicate the front entry carrying the back's state,
/// and is otherwise conforming.
///
/// The expected message names the **Err** path, which is the first of check 9's two
/// `try_pop_front_if` recording predicates to run. `TRY_POP_FRONT_IF_PREDICATES_ON_THE_BACK` is
/// the same site caught by the span half; this is a predicate shown the right token at a lexer
/// position it never occupied.
///
/// Scoped to `try_pop_front_if` in #183's ninth round, when the declining `pop_front_if` arm
/// gained a reader and became the first recording predicate in the check. This mutant used to
/// skew both methods and could only ever report at whichever ran first, so the `pop_front_if`
/// half was coverage-shaped and never fired; scoping it here keeps this cell on the site its
/// `expected` has always named, and [`cache_kit_catches_a_declining_predicate_shown_the_backs_state`]
/// pins the other method for real.
#[test]
#[should_panic(
  expected = "entry-state] pop-front-if try_pop_front_if()'s Err-path predicate argument"
)]
fn cache_kit_catches_a_predicate_shown_the_backs_state() {
  run_queue::<PREDICATE_ARG_STATE_SKEW>();
}

/// Mutant 15 (added by the #183 cross-model gate): the predicate argument's **token** half, for
/// the reason [`cache_kit_catches_a_refusal_that_swaps_in_a_residents_token`] exists.
#[test]
#[should_panic(
  expected = "entry-token] pop-front-if try_pop_front_if()'s Err-path predicate argument"
)]
fn cache_kit_catches_a_predicate_shown_the_backs_token() {
  run_queue::<PREDICATE_ARG_TOKEN_SKEW>();
}

/// Mutant 16, from #183's own adversarial review: `peek` caches a lookahead frontier and every
/// later pop on that instance reports its `L::State` instead of the removed entry's.
///
/// This is the #183 defect class surviving **inside** the #183 fix, on the peek-then-consume path
/// the issue's severity argument rests on. It is also the one site the PR's residual-coverage
/// argument did not cover: that argument is about comparisons reached through a return path some
/// other mutant already corrupts, and this one was reached by nothing, because
/// `check_peek_across_mutations` bound its popped value and asserted `is_some()` on it. **A
/// discarded value cannot be pinned by a mutant on any return path** — the only fix is to compare
/// it, which is what the drain now does.
///
/// The expected message names the drain's **first** pop. Every other drain in the kit pops an
/// instance that has never been peeked, so the frontier is unset and this fixture is conforming at
/// all of them — which is what makes the `expected` string a pin on the across-mutations drain
/// specifically, and not on `check_pop_order`'s.
///
/// Only the state half is a cell of its own, and deliberately. The comparison goes through the
/// shared [`CacheHarness::assert_entry_eq`](super::cache) helper, so the removable units here are
/// the whole call — which this cell catches — and the helper's token branch, which seven cells
/// above already pin. A token sibling would re-pin a branch that is pinned, not a site that is
/// not; the state half is the one to spend the cell on, since `L::State` is what a resume rebuilds
/// the lexer from.
#[test]
#[should_panic(expected = "entry-state] bounded-peek/across-mutations pop_front() #0")]
fn cache_kit_catches_a_peek_that_poisons_the_state_of_later_pops() {
  run_queue::<POST_PEEK_POP_STATE_SKEW>();
}

/// Mutant 17, from #183's third review round: the push stores perfectly and returns an `Ok`
/// reference assembled from a different resident.
///
/// Rounds 2 and 3 found the same class in two places — a value a `Cache` method handed back that
/// the kit reduced to presence: first a popped entry, then an accepted push's reference. So the
/// kit now reads **every** such value, and this cell is what keeps the push half of that from
/// being a check nothing can fail. The contract is explicit — `Ok` carries *a reference to the
/// cached token* — and the violation is invisible to every other oracle here, because they all
/// observe storage and this cell's storage is flawless.
///
/// The expected message names `push_back #1` in check 3, which is the first accepted push in the
/// whole kit that lands with a neighbour already resident: every earlier one goes into an empty
/// cache, where the entry pushed and the entry beside it are the same one and this cell is
/// conforming.
///
/// One cell for both arms, because both route through the kit's single accepted-push comparison —
/// the same granularity argument the two rounds before this one recorded: the removable units are
/// that one call, which this pins, and the shared entry helper's branches, which nine cells above
/// already pin.
#[test]
#[should_panic(expected = "entry-state] accepted-push push_back #1")]
fn cache_kit_catches_an_accepted_push_that_returns_the_wrong_reference() {
  run_queue::<WRONG_ACCEPTED_PUSH_REF>();
}

/// Mutant 18, from #183's fourth review round, and the one that finishes the class: a push
/// refused with room to spare that also corrupts the entry it hands back.
///
/// The `accepted_push_*` wrappers used to take `Err`, panic on the refusal and drop the returned
/// value — the last `Cache` return in the kit read for its shape and not its contents. This cell
/// is what makes the fix bite, and the `expected` string is the evidence for **which** assertion
/// fires: `entry-state] unexpected-refusal`, the entry comparison, not the refusal panic that
/// follows it. Without the comparison the refusal message would be all that ever appeared.
///
/// It is a claim-accuracy cell rather than a coverage one, and says so plainly: reaching it needs
/// a refusal the contract forbids, which the kit already fails on. No cache is certified because
/// of this. What was untrue before it is the kit's claim that every value a `Cache` method hands
/// back is compared or is a documented exception — and an enumeration with an unlisted third case
/// is the same defect class as an oracle that does not look, one level up.
#[test]
#[should_panic(
  expected = "entry-state] unexpected-refusal push_back #1 building an interleaved residency"
)]
fn cache_kit_catches_an_unwarranted_refusal_that_corrupts_the_returned_entry() {
  run_queue::<UNEXPECTED_REFUSAL_ENTRY_SWAP>();
}

/// Mutant 19, from #183's fifth review round: the **mixed** `push_back` site.
///
/// Round 4 put the unexpected-refusal comparison in the `accepted_push_*` wrappers, which was
/// right and incomplete — check 3's push never goes through a wrapper, because a refusal there is
/// sometimes legitimate. The same call, two meanings, and the unwarranted one asserted its way
/// out before the entry was read. The comparison now lives in `push_back_expecting`, told which
/// meaning applies by `must_accept`, and this cell is what makes that bite.
///
/// The `expected` string carries a trailing colon so it pins **this** site: mutant 18's message
/// continues `push_back #1 building an interleaved residency`, and a bare `#1` would match both.
#[test]
#[should_panic(expected = "entry-state] unexpected-refusal push_back #1:")]
fn cache_kit_catches_an_early_push_back_refusal_that_corrupts_the_returned_entry() {
  run_queue::<EARLY_PUSH_BACK_REFUSAL_ENTRY_SWAP>();
}

/// Mutant 20: the prepend twin of mutant 19, at check 5's mixed site.
#[test]
#[should_panic(expected = "entry-state] unexpected-refusal push_front #0:")]
fn cache_kit_catches_an_early_push_front_refusal_that_corrupts_the_returned_entry() {
  run_queue::<EARLY_PUSH_FRONT_REFUSAL_ENTRY_SWAP>();
}

/// Mutant 21, from #183's ninth review round: the **declining** `pop_front_if` predicate is shown
/// the back's `L::State`, and everything else about the call conforms.
///
/// The round's finding was a classification error, not a missing assertion. Check 9's
/// false-predicate arm sat among the kit's absence exceptions — "it returns `None`, so there is
/// no entry to compare" — which is true of the RETURN and says nothing about the ARGUMENT.
/// `pop_front_if` hands caller code a real entry *before* it learns the answer, so a cache can
/// answer `None` correctly, remove nothing, keep every span in place, and still run the caller's
/// validation predicate against a lexer position that entry never occupied.
///
/// The `expected` string names `false-predicate argument`, which pins the declining arm
/// specifically: `POP_FRONT_IF_PREDICATE_ARG_STATE_SKEW` is wrong on every `pop_front_if` arm —
/// `F: FnOnce` settles the entry before the answer exists — and this is simply the first of them
/// the kit now reads.
///
/// It fails on the **entry comparison**, not on residency, and that is the whole point of the
/// cell: `WRONG_POP_FRONT_IF` already covers a declining call that removes something. This one
/// removes nothing, so every residency and return oracle in check 9 agrees it conformed.
#[test]
#[should_panic(expected = "entry-state] pop-front-if pop_front_if()'s false-predicate argument")]
fn cache_kit_catches_a_declining_predicate_shown_the_backs_state() {
  run_queue::<POP_FRONT_IF_PREDICATE_ARG_STATE_SKEW>();
}

// ── CACHE_CALL_CENSUS ────────────────────────────────────────────────────────────────
//
// Four review rounds of #183 found the same defect four times: a value a `Cache` method handed
// the kit that the kit reduced to `is_ok()`/`is_some()` and never compared. R1 a popped entry,
// R2 the same on the peek-then-consume path, R3 every accepted push's `Ok` reference, R4/R5 the
// `Err` of a refusal at the two sites where a refusal may be either warranted or not.
//
// Each round the fix was correct and each round the *method* was the same: enumerate the sites
// by hand and argue the residue is fine. Four hand enumerations, four misses. The enumeration by
// **shape** was complete — `Some` / `None` / `Ok` / `Err`-expected / `Err`-unexpected, and there
// is no sixth — but the enumeration by **site** was not, because a site can be two shapes at
// once and a census organised by shape cannot see that.
//
// So this is the guard that replaces the argument: a mechanical scan of the kit's own source
// that fails, naming the function and line, the moment a `Cache` call appears anywhere it is not
// registered below. It cannot prove a value is compared — Rust has no call-site reflection, and
// `RECORD_CENSUS` and `SETTLE_CENSUS` live with the same limit — but it makes the *existence* of
// a site impossible to overlook, which is the step that kept failing.

/// The `Cache` methods whose return value carries an entry, and every spelling the scanner
/// recognises for a call to one.
///
/// Two spellings per method: the method-call form (`cache.pop_front(`) and the qualified form
/// (`Cache::pop_front(`, `C::pop_front(`, `<C as Cache<..>>::pop_front(`, `Self::pop_front(` —
/// all of which end in `::pop_front(`). The qualified form was invisible to this census until
/// #183's sixth review round, which is exactly the kind of gap the bounds note below exists to
/// keep honest rather than to pretend away.
///
/// `len`, `remaining`, `is_empty`, `span`, `front_span`, `back_span` and `clear` are deliberately
/// absent: they return counts, spans or nothing, so there is no entry behind them for an oracle
/// to fail to compare.
const GUARDED_METHODS: &[&str] = &[
  "push_back",
  "push_front",
  "pop_front",
  "pop_back",
  "pop_front_if",
  "try_pop_front_if",
  "peek",
  "peek_one",
  "push_many",
  "front",
  "back",
];

/// What a registered site does with the value, as the key its anchor comment must carry.
const ROUTED: &str = "routed";
const COMPARED_IN_PLACE: &str = "compared-in-place";
const NOT_A_CACHE_CALL: &str = "not-a-cache-call";
const ABSENCE: &str = "absence";

/// Every guarded call the kit makes: `(function, method, occurrences, kind, reason)`.
///
/// The count catches an added or removed call. It cannot catch a **swap** — delete one guarded
/// call and add a different, uncompared one in the same function and the count is unchanged —
/// which is why every registered call also carries an anchor comment at the site, and why the
/// scanner requires one. The count is the inventory; the anchor is the identity.
///
/// `kind` is checked against the anchor, so a site cannot be silently reclassified from
/// `absence` to `routed` (or the reverse) by editing only one of the two places.
const CALL_SITES: &[(&str, &str, &str, usize, &str, &str)] = &[
  // ── routed: the comparison lives in the callee ──────────────────────────────────
  (
    "push_back_expecting",
    "push_back",
    "cache",
    1,
    ROUTED,
    "Compares the Ok reference against the offered entry; on a refusal, hands it to the caller when the refusal is legitimate and to `unexpected_refusal` when it is not.",
  ),
  (
    "push_front_expecting",
    "push_front",
    "cache",
    1,
    ROUTED,
    "The prepend twin of `push_back_expecting`.",
  ),
  (
    "drained_front",
    "pop_front",
    "cache",
    1,
    ROUTED,
    "Compares the popped entry against the entry the kit knows is next off the front.",
  ),
  (
    "drained_back",
    "pop_back",
    "cache",
    1,
    ROUTED,
    "The same at the restore-path end, with RESTORE_NOTE implied by the end rather than passed in.",
  ),
  (
    "assert_front_entry",
    "front",
    "cache",
    1,
    ROUTED,
    "Compares the front reference in full.",
  ),
  (
    "assert_back_entry",
    "back",
    "cache",
    1,
    ROUTED,
    "Compares the back reference in full; this is the entry `InputRef::resume` rebuilds a lexer from.",
  ),
  (
    "peeked_entries_through",
    "peek",
    "cache",
    1,
    ROUTED,
    "Flattens every peeked entry to the triple the kit compares.",
  ),
  (
    "peeked_entries_after_prefill",
    "peek",
    "cache",
    1,
    ROUTED,
    "The prefilled-buffer shape, same flattening.",
  ),
  (
    "peeked_one",
    "peek_one",
    "cache",
    1,
    ROUTED,
    "Flattens to the triple, so the caller compares an entry and not a span.",
  ),
  (
    "recording_pop_front_if",
    "pop_front_if",
    "cache",
    1,
    ROUTED,
    "Owns the recording closure, so the predicate's argument is compared unconditionally and cannot be discarded by an edit at a call site.",
  ),
  (
    "recording_try_pop_front_if",
    "try_pop_front_if",
    "cache",
    1,
    ROUTED,
    "The fallible twin.",
  ),
  // ── compared in place ───────────────────────────────────────────────────────────
  (
    "check_push_many",
    "push_many",
    "cache",
    1,
    COMPARED_IN_PLACE,
    "The overflow iterator is collected and every entry compared against `corpus[cap..]` on the lines below.",
  ),
  // ── not a Cache call ────────────────────────────────────────────────────────────
  (
    "peeked_entries_after_prefill",
    "push_back",
    "buf",
    1,
    NOT_A_CACHE_CALL,
    "`GenericArrayDeque::push_back` on the kit's OWN peek buffer, loading the prefill before `peek` is invoked. Registered rather than special-cased in the scanner, because a scanner that guessed at receivers would be the kind of check that passes by not looking.",
  ),
  // ── absence is the law under test ───────────────────────────────────────────────
  (
    "assert_empty",
    "front",
    "cache",
    1,
    ABSENCE,
    "An empty cache must not answer.",
  ),
  (
    "assert_empty",
    "back",
    "cache",
    1,
    ABSENCE,
    "An empty cache must not answer.",
  ),
  (
    "assert_empty",
    "pop_front",
    "cache",
    1,
    ABSENCE,
    "A pop must not answer on an empty cache; this probe is what catches RESURRECTING_CLEAR.",
  ),
  (
    "assert_empty",
    "pop_back",
    "cache",
    1,
    ABSENCE,
    "As above, at the other end.",
  ),
  (
    "assert_empty",
    "peek_one",
    "cache",
    1,
    ABSENCE,
    "peek_one must name nothing where there is no front.",
  ),
  (
    "check_pop_order",
    "pop_front",
    "cache",
    1,
    ABSENCE,
    "After the full drain, nothing is left to return.",
  ),
  (
    "check_pop_order",
    "pop_back",
    "cache",
    1,
    ABSENCE,
    "As above, at the other end.",
  ),
  (
    "check_push_front",
    "pop_front",
    "mixed",
    2,
    ABSENCE,
    "The full-order and interleaved-order drains each end by asserting nothing is left.",
  ),
  (
    "check_push_front",
    "pop_back",
    "mixed",
    1,
    ABSENCE,
    "The from-the-back drain ends the same way.",
  ),
  (
    "check_pop_front_if",
    "pop_front_if",
    "empty",
    1,
    ABSENCE,
    "The empty-cache probe: the predicate must not run at all, and the method must answer None.",
  ),
  (
    "check_pop_front_if",
    "try_pop_front_if",
    "empty2",
    1,
    ABSENCE,
    "The empty-cache probe.",
  ),
];

/// The marker every registered call carries on the comment line(s) directly above it.
const ANCHOR: &str = "CACHE_CALL_CENSUS:";

/// The macros the censused file may invoke.
///
/// `syn` hands the walk a macro **invocation**, never its expansion, so a macro this list does not
/// name could expand to anything — `cache_call!(cache)` names nothing guarded, parses as an
/// expression list, and expands to `cache.pop_front()`. #183's eighth round found exactly that,
/// and it recorded neither a call nor a warning. Rather than widen the census's bound to admit it,
/// the bound is made **true**: the census governs one file, so requiring every macro in that file
/// to come from a closed list is enforceable, and then "a guarded call appears literally in the
/// source" is a wall rather than a caveat.
///
/// The admission criterion is one property, and it is the reason walking the *body* is sound for
/// everything here: **the expansion adds no call the author chose.** Stated that precisely on
/// purpose — "adds no call at all" would be wider than what holds, and a census whose own
/// justification overclaims is #183's defect one level in. These macros do add calls: `assert_eq!`
/// inserts `PartialEq::eq`, the `format_args!` family inserts `Display::fmt`. But which calls they
/// add is fixed by std, not by what is written at the invocation, and none is a `Cache` method. So
/// every *guarded* call in the expansion is one that appears literally in the tokens, which is
/// exactly what the walk reads. A user macro chooses its own expansion, which is why none is
/// admitted.
///
/// `macro_rules` is deliberately absent, and that absence is what makes *defining* a macro in the
/// censused file drift: a definition is an `Item::Macro` whose path is `macro_rules`, so it fails
/// this list on the way in. A macro defined elsewhere in the crate is covered too — its
/// *invocation* is what has to appear here, and only the invocation is in this file.
///
/// Matched on the **whole** path, not the last segment: `evil::assert!` is not `assert!`. So a
/// qualified spelling of an admitted macro (`std::format!`) is refused until it is written down
/// here, which is a red rather than a hole, and the conscious act is the point.
const MACRO_ALLOWLIST: &[&str] = &[
  "assert",
  "assert_eq",
  "assert_ne",
  "format",
  "format_args",
  "matches",
  "panic",
  "unreachable",
];

/// The attributes the censused file may carry.
///
/// The same hole one level up, and the reason it is a separate list: an attribute macro rewrites
/// the item the walk is reading, and `syn::visit` never routes an attribute through
/// `CensusVisitor::visit_macro` — `Attribute` holds a `Meta`, not a `Macro` — so an
/// attribute-macro invocation is not merely unexpanded, it is entirely outside the visitor.
///
/// Everything admitted here is compiler-built-in and cannot introduce a call: `doc` is inert,
/// `must_use`, `inline` and `cold` annotate, `allow`/`expect` govern lints, `cfg` can only
/// *remove* an item. `derive` is admitted because it is built-in, but what it expands is not, so
/// its arguments go through [`DERIVE_ALLOWLIST`] separately.
///
/// `cfg_attr` is deliberately absent: it applies an arbitrary attribute, so admitting it would
/// admit whatever it names.
const ATTR_ALLOWLIST: &[&str] = &[
  "allow", "cfg", "cold", "derive", "doc", "expect", "inline", "must_use",
];

/// The derive macros the censused file may ask for — the built-ins, which expand to `impl` blocks
/// over the deriving type's own fields and cannot reach a `Cache` method.
///
/// A third-party derive can expand to anything, which is the whole reason `derive`'s presence in
/// [`ATTR_ALLOWLIST`] is not enough on its own.
const DERIVE_ALLOWLIST: &[&str] = &[
  "Clone",
  "Copy",
  "Debug",
  "Default",
  "Eq",
  "Hash",
  "Ord",
  "PartialEq",
  "PartialOrd",
];

/// One scanned call: enclosing function, method, receiver, 1-based line, and the anchor kind
/// bound to it.
#[derive(Debug, PartialEq, Eq)]
struct CensusCall {
  function: std::string::String,
  method: &'static str,
  receiver: std::string::String,
  line: usize,
  anchor: Option<std::string::String>,
}

/// A macro invocation whose body the walk could not read as an expression list, so nothing inside
/// it was visited.
#[derive(Debug, PartialEq, Eq)]
struct OpaqueMacro {
  function: std::string::String,
  path: std::string::String,
  line: usize,
  /// Whether the unread tokens name a guarded method — the loud case. Reported either way: an
  /// unwalked body is a region the census did not look at, and saying so is the difference
  /// between a bound and an assumption.
  names_guarded: bool,
}

/// What one scan of a source file found.
#[derive(Debug, Default)]
struct CensusScan {
  calls: std::vec::Vec<CensusCall>,
  /// Anchors with no call bound to them — dead markers.
  orphan_anchors: std::vec::Vec<usize>,
  /// A guarded method named as a **function item** rather than called: `Cache::pop_front` with
  /// no argument list, which a later call through the resulting pointer would hide.
  function_items: std::vec::Vec<(std::string::String, usize)>,
  /// A macro whose body does not parse as an expression list, so the walk saw none of it.
  opaque_macros: std::vec::Vec<OpaqueMacro>,
  /// A macro invocation whose path is not in [`MACRO_ALLOWLIST`] — its expansion is unknown, and
  /// an unknown expansion can contain a guarded call that appears nowhere in the source.
  unknown_macros: std::vec::Vec<(std::string::String, std::string::String, usize)>,
  /// An attribute whose path is not in [`ATTR_ALLOWLIST`], or a `derive` argument not in
  /// [`DERIVE_ALLOWLIST`]: an attribute macro rewrites the item the walk is reading.
  unknown_attrs: std::vec::Vec<(std::string::String, std::string::String, usize)>,
}

/// The receiver a call is made on, as a short readable name.
///
/// This is what distinguishes `buf.push_back(…)` — the kit's own peek buffer — from
/// `cache.push_back(…)`, which is a real `Cache` return value. The substring census could not
/// tell them apart at all, so its `not-a-cache-call` row was a promise rather than a check.
fn census_receiver(expr: &syn::Expr) -> std::string::String {
  match expr {
    syn::Expr::Path(p) => p
      .path
      .segments
      .last()
      .map_or_else(|| "<path>".into(), |s| census_ident(&s.ident)),
    syn::Expr::Field(f) => match &f.member {
      syn::Member::Named(n) => std::format!("{}.{}", census_receiver(&f.base), census_ident(n)),
      syn::Member::Unnamed(i) => std::format!("{}.{}", census_receiver(&f.base), i.index),
    },
    syn::Expr::Reference(r) => census_receiver(&r.expr),
    syn::Expr::Paren(p) => census_receiver(&p.expr),
    syn::Expr::Unary(u) => census_receiver(&u.expr),
    syn::Expr::MethodCall(m) => std::format!(
      "{}.{}()",
      census_receiver(&m.receiver),
      census_ident(&m.method)
    ),
    syn::Expr::Call(c) => std::format!("{}()", census_receiver(&c.func)),
    _ => "<expr>".into(),
  }
}

/// An identifier as the census compares it, with the `r#` of a raw identifier stripped.
///
/// `Ident::to_string` **keeps** the `r#`, and #183's eighth round found what that costs:
/// `cache.r#pop_front()` and `Cache::r#pop_front(cache)` are ordinary calls to a guarded method
/// that no comparison against [`GUARDED_METHODS`] can match. The source parses, so they were not
/// reported as an opaque macro either — simply invisible, in every form. It cuts the other way
/// too: `r#cache.pop_front()` would have missed the `cache` row in [`CALL_SITES`] and been
/// reported as an unregistered receiver.
///
/// Deliberately a string strip rather than `syn::ext::IdentExt::unraw`, which rebuilds the
/// identifier through `Ident::new` — and `Ident::new` panics on a keyword. `unraw` on a
/// `r#type` field anywhere in the censused file would take the whole census down with it, and
/// this function is called on every identifier the walk stringifies, not only on guarded ones.
fn census_ident(ident: &proc_macro2::Ident) -> std::string::String {
  let text = ident.to_string();
  match text.strip_prefix("r#") {
    Some(bare) => bare.to_string(),
    None => text,
  }
}

/// A path as `a::b::c`, each segment unrawed, generic arguments dropped.
fn census_path(path: &syn::Path) -> std::string::String {
  let mut out = std::string::String::new();
  for (i, seg) in path.segments.iter().enumerate() {
    if i > 0 || path.leading_colon.is_some() {
      out.push_str("::");
    }
    out.push_str(&census_ident(&seg.ident));
  }
  out
}

/// Whether a token stream names a guarded method as an **identifier**.
///
/// Identifiers only: the kit's failure messages are full of `pop_front` in prose, and a string
/// literal that mentions one is not a call. Unrawed for the reason [`census_ident`] gives — a
/// `r#pop_front` buried in tokens the walk could not parse is the quietest form of all.
fn census_tokens_name_a_guarded_method(tokens: &proc_macro2::TokenStream) -> bool {
  tokens.clone().into_iter().any(|tt| match tt {
    proc_macro2::TokenTree::Ident(id) => {
      let name = census_ident(&id);
      GUARDED_METHODS.contains(&name.as_str())
    }
    proc_macro2::TokenTree::Group(g) => census_tokens_name_a_guarded_method(&g.stream()),
    _ => false,
  })
}

/// The visitor. Walks the expression tree, so call **syntax** is exact: whitespace and line
/// breaks around `.` and `::` are gone by the time it runs, a turbofish is part of the call
/// expression rather than a spelling to match, and a path that is merely *named* is an
/// `ExprPath` and not an `ExprCall`.
struct CensusVisitor {
  scope: std::vec::Vec<std::string::String>,
  found: CensusScan,
}

impl CensusVisitor {
  fn here(&self) -> std::string::String {
    self
      .scope
      .last()
      .cloned()
      .unwrap_or_else(|| "<file scope>".into())
  }

  fn guarded(ident: &proc_macro2::Ident) -> Option<&'static str> {
    let name = census_ident(ident);
    GUARDED_METHODS.iter().copied().find(|m| *m == name)
  }
}

impl<'ast> syn::visit::Visit<'ast> for CensusVisitor {
  fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
    self.scope.push(census_ident(&node.sig.ident));
    syn::visit::visit_item_fn(self, node);
    self.scope.pop();
  }

  fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
    self.scope.push(census_ident(&node.sig.ident));
    syn::visit::visit_impl_item_fn(self, node);
    self.scope.pop();
  }

  /// A default body in a trait definition. There is none in the censused file today, and this is
  /// here so that adding one is not quietly wrong: without it a call in a default body would be
  /// attributed to the enclosing scope, which is not merely a bad label — the identity check
  /// matches on the function name, so it could match a `CALL_SITES` row belonging to someone else.
  fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
    self.scope.push(census_ident(&node.sig.ident));
    syn::visit::visit_trait_item_fn(self, node);
    self.scope.pop();
  }

  fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
    if let Some(method) = Self::guarded(&node.method) {
      self.found.calls.push(CensusCall {
        function: self.here(),
        method,
        receiver: census_receiver(&node.receiver),
        line: node.method.span().start().line,
        anchor: None,
      });
    }
    syn::visit::visit_expr_method_call(self, node);
  }

  fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
    // A qualified call — `Cache::push_back(cache, tok)`, `<C as Cache<..>>::push_back(..)`. Its
    // receiver is the first argument, so it lands in the table beside the method-call spelling
    // rather than in a category of its own.
    let mut counted = false;
    if let syn::Expr::Path(path) = node.func.as_ref()
      && let Some(last) = path.path.segments.last()
      && let Some(method) = Self::guarded(&last.ident)
    {
      self.found.calls.push(CensusCall {
        function: self.here(),
        method,
        receiver: node
          .args
          .first()
          .map_or_else(|| "<none>".into(), census_receiver),
        line: last.ident.span().start().line,
        anchor: None,
      });
      counted = true;
    }
    // Recurse into the arguments either way; skip the callee path when it was the call, so it is
    // not also reported as a function item.
    if !counted {
      self.visit_expr(&node.func);
    }
    for arg in &node.args {
      self.visit_expr(arg);
    }
    // This is the one visitor that recurses by hand rather than delegating, so it is the one
    // place an `attrs` field can be dropped on the floor. `visit_attribute` is where the
    // attribute-macro bound is enforced, so skipping it here would be a hole in that bound
    // specifically — cheap to close, and invisible if it were not.
    for attr in &node.attrs {
      self.visit_attribute(attr);
    }
  }

  fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
    // Two or more segments only: a method function item is always reached through a type or
    // trait path (`Cache::pop_front`). A bare single-segment path is a local — the kit has
    // variables called `back` and `front` — and calling those function items would be nonsense.
    if node.path.segments.len() >= 2
      && let Some(last) = node.path.segments.last()
      && let Some(_method) = Self::guarded(&last.ident)
    {
      // Named but not called: a function item. Not a call — reporting it as one is how the
      // substring census spent a registered count on `Cache::try_pop_front_if::<E, _>` while the
      // real call through it stayed invisible — but not nothing either, so it is surfaced.
      self
        .found
        .function_items
        .push((self.here(), last.ident.span().start().line));
    }
    syn::visit::visit_expr_path(self, node);
  }

  fn visit_macro(&mut self, node: &'ast syn::Macro) {
    // `syn` hands back a macro's tokens, not its expansion — so this method has two jobs, and
    // #183's eighth round found that only the first was being done.
    //
    // The first is to walk what IS here: most of the kit's guarded calls sit inside `assert!`,
    // whose body is an expression list, so parse that and visit it like any other expression.
    //
    // The second is the expansion, which no amount of walking the tokens reaches.
    // `cache_call!(cache)` names nothing guarded, parses cleanly as one expression, and expands
    // to `cache.pop_front()`. Walking its tokens records a call that is not there and misses the
    // one that is. The answer is not to look harder at the tokens — it is to refuse a macro whose
    // expansion the census cannot account for, which MACRO_ALLOWLIST makes possible because this
    // census governs exactly one file.
    let path = census_path(&node.path);
    let line = node
      .path
      .segments
      .last()
      .map_or(0, |s| s.ident.span().start().line);
    if !MACRO_ALLOWLIST.contains(&path.as_str()) {
      // Unknown expansion. Reported and NOT walked: the tokens of a macro whose meaning is
      // unknown are not evidence about anything, in either direction.
      self.found.unknown_macros.push((self.here(), path, line));
      return;
    }

    let parsed = node
      .parse_body_with(syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated);
    match parsed {
      Ok(exprs) => {
        for expr in &exprs {
          self.visit_expr(expr);
        }
      }
      Err(_) => {
        // Allowlisted, so the expansion introduces no call of its own — but the body was not
        // walked, so any call written inside it went unseen. That is a region the census did not
        // look at, and it is reported as one whether or not the tokens name something guarded.
        self.found.opaque_macros.push(OpaqueMacro {
          function: self.here(),
          path,
          line,
          names_guarded: census_tokens_name_a_guarded_method(&node.tokens),
        });
      }
    }
  }

  fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
    // An attribute never reaches `visit_macro`: `Attribute` holds a `Meta`, not a `Macro`, so an
    // attribute macro is not merely unexpanded — it is outside the visitor entirely, and it
    // rewrites the very item the walk is reading. The bound is closed the same way, by requiring
    // every attribute in the censused file to be one that cannot introduce a call.
    let path = census_path(node.path());
    let line = node
      .path()
      .segments
      .last()
      .map_or(0, |s| s.ident.span().start().line);
    if !ATTR_ALLOWLIST.contains(&path.as_str()) {
      self.found.unknown_attrs.push((self.here(), path, line));
      return;
    }
    // `derive` is built-in; what it expands is not. A third-party derive expands to items of its
    // own choosing, so the arguments are checked against their own list.
    if path == "derive"
      && let Ok(paths) = node
        .parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
    {
      for p in &paths {
        let name = census_path(p);
        if !DERIVE_ALLOWLIST.contains(&name.as_str()) {
          self
            .found
            .unknown_attrs
            .push((self.here(), std::format!("derive({name})"), line));
        }
      }
    }
    syn::visit::visit_attribute(self, node);
  }
}

/// Parse `src`, collect every guarded call, and bind each to the anchor comment above it.
///
/// The parse gives every call a **line**, which is what lets an anchor bind to a call rather
/// than to a comment block: each anchor is consumed by at most one call, so a second call added
/// under an existing marker has no anchor of its own and is reported.
fn census_scan(src: &str) -> CensusScan {
  let file = syn::parse_file(src).expect("CACHE_CALL_CENSUS: the censused source must parse");
  let mut visitor = CensusVisitor {
    scope: std::vec::Vec::new(),
    found: CensusScan::default(),
  };
  syn::visit::Visit::visit_file(&mut visitor, &file);
  let mut found = visitor.found;
  // Everything is reported by line, so everything is sorted by line: the drift list is read by a
  // human next to the file it names.
  found.calls.sort_by_key(|c| c.line);
  found.function_items.sort_by_key(|&(_, line)| line);
  found.opaque_macros.sort_by_key(|m| m.line);
  found.unknown_macros.sort_by_key(|&(_, _, line)| line);
  found.unknown_attrs.sort_by_key(|&(_, _, line)| line);

  let lines: std::vec::Vec<&str> = src.lines().collect();
  let anchor_at = |i: usize| -> Option<std::string::String> {
    let t = lines.get(i)?.trim_start();
    if !t.starts_with("//") {
      return None;
    }
    let pos = t.find(ANCHOR)?;
    Some(t[pos + ANCHOR.len()..].trim().to_string())
  };
  let mut consumed: std::vec::Vec<usize> = std::vec::Vec::new();
  for call in &mut found.calls {
    let mut probe = call.line.saturating_sub(1); // 0-based index of the line above
    while probe > 0 {
      probe -= 1;
      let Some(text) = lines.get(probe) else { break };
      if !text.trim_start().starts_with("//") {
        break;
      }
      if let Some(kind) = anchor_at(probe)
        && !consumed.contains(&probe)
      {
        consumed.push(probe);
        call.anchor = Some(kind);
        break;
      }
    }
  }
  for (i, line) in lines.iter().enumerate() {
    if line.trim_start().starts_with("//") && line.contains(ANCHOR) && !consumed.contains(&i) {
      found.orphan_anchors.push(i + 1);
    }
  }
  found.orphan_anchors.sort_unstable();
  found
}

/// CACHE_CALL_CENSUS — no `Cache` value reaches the kit from a site this table does not know
/// about, and no registered site can be swapped for a different call without saying so.
///
/// # What this census deliberately does not check
///
/// Stated rather than implied, because **this whole change exists because the kit made a coverage
/// claim wider than what it checked**. A census that quietly claimed totality would be the same
/// defect at one remove; the standard here is calibrated, not total.
///
/// Three of the bounds this note carried while the census was a substring scanner are **gone**,
/// because the parse made them false, and a bound that is no longer true is the same defect as a
/// claim that never was. For the record, they were: that a call form outside a fixed list of
/// spellings was invisible; that an anchor bound to a comment block rather than to a call, so a
/// second call could shelter under an existing marker; and that nothing checked the receiver, so
/// a real `cache.push_back` could sit under the `not-a-cache-call` row. The walk closes all three
/// — layout and turbofish are gone before it runs, every call carries its own line so an anchor
/// binds to one call, and `ExprMethodCall` carries the receiver.
///
/// A fourth is gone too, and it went the other way — not because the parse made it false, but
/// because it *was* false. It read: a call can hide only in a macro body the parse cannot read.
/// #183's eighth round falsified it with a body that parses perfectly: `cache_call!(cache)` names
/// nothing guarded, is one clean expression, and expands to `cache.pop_front()`. The bound was not
/// about *unparseable* bodies at all; it was about **expansion**, which `syn` never performs.
/// Widening the wording to admit that would have been honest and useless. Instead the claim is
/// made true: **every macro invocation and every attribute in this file must come from a closed
/// list** ([`MACRO_ALLOWLIST`], [`ATTR_ALLOWLIST`], [`DERIVE_ALLOWLIST`]), each entry admitted only
/// because its expansion cannot introduce a call of its own. A macro that could generate one
/// cannot appear here at all — including a `macro_rules` definition, whose path is not on the
/// list. So a guarded call must appear *literally* in this source, and that is now a wall rather
/// than a caveat.
///
/// What is left:
///
/// * **A macro body the parse cannot read is not walked**, and the residue is now this and nothing
///   more: the macro is one of the eight, so its expansion adds no call, but a call written
///   *inside* its body went unvisited. It is not silent — the body is **reported**, whether or not
///   its tokens name a guarded method, because a region the census did not look at is exactly what
///   it must not pass over quietly. See [`cache_call_census_reports_a_macro_body_it_cannot_read`].
/// * **It cannot prove a routed helper actually compares.** It checks where a call is, what it is
///   made on, and what its anchor says — not what the callee does with the value. That is what
///   the twenty mutant cells and the ablation are for; this census is defence in depth behind
///   them, not the primary protection.
/// * **An author who adds both a call and its anchor has registered it.** The census is a speed
///   bump that forces a conscious act and a reviewable diff, in the same way `RECORD_CENSUS` and
///   `SETTLE_CENSUS` are.
/// * **Scope is this file.** `cache_tests.rs`'s own fixture calls `Cache` methods constantly — it
///   *is* the cache under test — and is not scanned.
///
/// A guarded method named as a **function item** (`Cache::pop_front` with no argument list) is
/// not a bound and not a call: it is reported as drift, because a later call through that pointer
/// would be invisible. The substring census counted such a path *as a call*, which spent the
/// registered count while the real call stayed hidden — a miss wearing the costume of coverage.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn cache_call_census_every_guarded_call_is_registered() {
  let found = census_scan(include_str!("cache.rs"));
  let drift = census_drift(&found);

  assert!(
    drift.is_empty(),
    "CACHE_CALL_CENSUS drift:\n  {}\n\nEvery call to a Cache method that returns an entry must \
     carry an anchor comment saying what happens to the value — routed through a comparing \
     helper, compared in place, or checked only for absence because absence is the law — and \
     must be registered in CALL_SITES with the same kind AND the same receiver. Six review \
     rounds of #183 each found one site that had been reasoned about and missed (grep \
     CACHE_CALL_CENSUS). Route the call, anchor it, and register it in the same commit. A macro \
     or attribute that is not allowlisted is refused rather than guessed at: syn does not expand, \
     so an unlisted expansion could carry a call that appears nowhere in the source.",
    drift.join("\n  ")
  );
}

/// Every way a scan can be drift, as the lines the failure prints.
///
/// Split out of the test above so it can be *called with a scan that is not clean*. It has to be:
/// on a conforming `cache.rs` not one of these seven arms executes, so nothing would otherwise
/// exercise the messages themselves — and a swapped placeholder in a failure message is invisible
/// exactly when it matters, which is the shape of defect this whole census exists to refuse.
/// [`cache_call_census_names_every_defect_it_reports`] drives all seven.
fn census_drift(found: &CensusScan) -> std::vec::Vec<std::string::String> {
  let mut drift = std::vec::Vec::new();

  // 1. Identity: every call carries an anchor of its own, on the right receiver, and the anchor
  //    agrees with the table.
  for call in &found.calls {
    let CensusCall {
      function: f,
      method: m,
      receiver: r,
      line,
      anchor,
    } = call;
    let registered = CALL_SITES
      .iter()
      .find(|(g, n, recv, _, _, _)| g == f && n == m && recv == r);
    match (anchor.as_deref(), registered) {
      (None, _) => drift.push(std::format!(
        "conformance/cache.rs:{line}: `{f}` calls `{r}.{m}` with no `// {ANCHOR} <kind>` anchor of its own"
      )),
      (Some(k), None) => drift.push(std::format!(
        "conformance/cache.rs:{line}: `{f}` calls `{r}.{m}` (anchored `{k}`) and is NOT in CALL_SITES for that receiver"
      )),
      (Some(k), Some((_, _, _, _, want_kind, _))) if k != *want_kind => drift.push(std::format!(
        "conformance/cache.rs:{line}: `{f}`'s `{r}.{m}` is anchored `{k}` but registered `{want_kind}`"
      )),
      (Some(_), Some(_)) => {}
    }
  }

  // 2. Inventory: the per-site counts match, in both directions.
  for (f, m, recv, want, _, _) in CALL_SITES {
    let got = found
      .calls
      .iter()
      .filter(|c| c.function == *f && c.method == *m && c.receiver == *recv)
      .count();
    if got != *want {
      drift.push(std::format!(
        "CACHE_CALL_CENSUS: `{f}` calls `{recv}.{m}` {got} time(s), registered for {want}"
      ));
    }
  }

  // 3. No anchor without a call bound to it — a marker nobody maintains reads like coverage.
  for line in &found.orphan_anchors {
    drift.push(std::format!(
      "conformance/cache.rs:{line}: a `{ANCHOR}` anchor with no call bound to it"
    ));
  }

  // 4. A guarded method named as a function item is not a call, and is not reported as one —
  //    but a later call through the pointer would be invisible, so naming one at all is drift.
  for (f, line) in &found.function_items {
    drift.push(std::format!(
      "conformance/cache.rs:{line}: `{f}` names a guarded method as a function item rather than calling it; a call through that pointer would be invisible to this census"
    ));
  }

  // 5. A macro whose expansion is not accounted for. `syn` never expands, so an unlisted macro
  //    could produce any call at all while its invocation names none — the R8 falsification.
  //    A `macro_rules` definition lands here too: its path is not on the list.
  for (f, path, line) in &found.unknown_macros {
    drift.push(std::format!(
      "conformance/cache.rs:{line}: `{f}` invokes `{path}!`, which is not in MACRO_ALLOWLIST; syn does not expand macros, so its expansion could contain a guarded call that appears nowhere in this source"
    ));
  }

  // 6. An attribute the walk cannot account for — and an attribute macro rewrites the very item
  //    being read, from outside the visitor entirely.
  for (f, path, line) in &found.unknown_attrs {
    drift.push(std::format!(
      "conformance/cache.rs:{line}: `{f}` carries `#[{path}]`, which is not allowlisted; an attribute macro rewrites the item this census is reading"
    ));
  }

  // 7. And the residue: an allowlisted macro whose body the parse could not read, so nothing
  //    inside it was walked.
  for OpaqueMacro {
    function: f,
    path,
    line,
    names_guarded,
  } in &found.opaque_macros
  {
    let tail = if *names_guarded {
      " — and its tokens name a guarded method"
    } else {
      ""
    };
    drift.push(std::format!(
      "conformance/cache.rs:{line}: `{f}`'s `{path}!` body does not parse as an expression list, so the census did not walk it{tail}"
    ));
  }

  drift
}

/// CACHE_CALL_CENSUS — the positive control: a census that cannot fail is not a census.
///
/// Every shape the walk exists to separate is planted here: the ones it must report, and the ones
/// it must **not**. The second half matters as much — a scanner that over-reports spends a
/// registered count on something that is not a call, which is a miss wearing the costume of
/// coverage, and is exactly how the substring census read `Cache::try_pop_front_if::<E, _>`.
#[test]
#[cfg_attr(
  miri,
  ignore = "parses a literal and walks it: no UB surface, and miri interprets every byte"
)]
fn cache_call_census_detects_every_defect_it_guards_against() {
  let planted = r#"
impl Thing {
  fn anchored_router(&self, cache: &mut C) {
    // CACHE_CALL_CENSUS: routed
    let _ = cache.pop_front();
  }

  fn spaced(&self, cache: &mut C) {
    let _ = cache . pop_back();
  }

  fn line_broken(&self, cache: &mut C) {
    let _ = cache
      .peek_one();
  }

  fn qualified_spaced(&self, cache: &mut C) {
    let _ = C :: push_front(cache, tok);
  }

  fn turbofish_call(&self, cache: &mut C) {
    let _ = cache.try_pop_front_if::<E, _>(|_| Err("no"));
  }

  fn inferred_turbofish(&self, cache: &mut C) {
    let _: Option<Result<T, &str>> = cache.try_pop_front_if(|_| Err("no"));
  }

  fn inside_a_macro(&self, cache: &mut C) {
    assert!(cache.front().is_none() && cache.back().is_none(), "both");
  }

  fn wrong_receiver(&self, cache: &mut C, buf: &mut Buf) {
    // CACHE_CALL_CENSUS: not-a-cache-call
    cache.push_back(tok);
  }

  fn function_item(&self) {
    let f = Cache::try_pop_front_if::<E, _>;
  }

  fn not_a_call(&self) {
    let _ = self.push_back_expecting_something;
  }

  fn orphan(&self) {
    // CACHE_CALL_CENSUS: routed
    let _ = 1;
  }

  fn raw_method(&self, cache: &mut C) {
    let _ = cache.r#pop_front();
  }

  fn raw_qualified(&self, cache: &mut C) {
    let _ = C::r#pop_back(cache);
  }

  fn raw_receiver(&self, r#cache: &mut C) {
    let _ = r#cache.peek::<W>(&mut buf);
  }

  fn raw_function_item(&self) {
    let f = Cache::r#peek_one;
  }

  fn parseable_macro(&self, cache: &mut C) {
    let _ = cache_call!(cache);
  }

  #[some_attribute_macro]
  fn attributed(&self) {}

  fn third_party_derive(&self) {
    #[derive(Clone, ThirdPartyDerive)]
    struct S;
  }
}
"#;
  let found = census_scan(planted);
  let seen: std::vec::Vec<(&str, &str, &str, usize, Option<&str>)> = found
    .calls
    .iter()
    .map(|c| {
      (
        c.function.as_str(),
        c.method,
        c.receiver.as_str(),
        c.line,
        c.anchor.as_deref(),
      )
    })
    .collect();

  // ── shapes the walk MUST report ──────────────────────────────────────────────────
  //
  // The first is the ordinary anchored call. The next three are the layouts the substring
  // matcher could not see, because Rust allows whitespace and newlines around `.` and `::` and
  // a text scan cannot. The turbofish pair is the #183-round-six mis-specification from both
  // sides: with the turbofish and without it.
  let must = [
    ("anchored_router", "pop_front", "cache", 5, Some("routed")),
    ("spaced", "pop_back", "cache", 9, None),
    ("line_broken", "peek_one", "cache", 14, None),
    ("qualified_spaced", "push_front", "cache", 18, None),
    ("turbofish_call", "try_pop_front_if", "cache", 22, None),
    ("inferred_turbofish", "try_pop_front_if", "cache", 26, None),
    // Inside `assert!`: `syn` hands back a macro's tokens, so the body is parsed as an
    // expression list and walked. Most of the kit's absence probes live here.
    ("inside_a_macro", "front", "cache", 30, None),
    ("inside_a_macro", "back", "cache", 30, None),
    // The receiver case the substring census could not express at all: a genuine
    // `cache.push_back` sitting under an anchor registered for the kit's own `buf.push_back`.
    // It is reported with receiver `cache`, so the table lookup for `buf` misses it.
    (
      "wrong_receiver",
      "push_back",
      "cache",
      35,
      Some("not-a-cache-call"),
    ),
    // The three raw-identifier forms, #183's eighth round. `Ident::to_string` keeps the `r#`, so
    // every one of these was a real call to a guarded method that matched nothing — and, because
    // the source parses, was not reported as an opaque macro or as drift either. Invisible, not
    // merely unclassified.
    ("raw_method", "pop_front", "cache", 52, None),
    ("raw_qualified", "pop_back", "cache", 56, None),
    // The same normalisation on the RECEIVER, which fails the other way: unrawed, this is the
    // `cache` the table knows; left raw, it would be reported as an unregistered `r#cache`.
    ("raw_receiver", "peek", "cache", 60, None),
  ];
  for want in must {
    assert!(
      seen.contains(&want),
      "CACHE_CALL_CENSUS misses {want:?} — it must report every legal spelling of a call.\nsaw: {seen:?}"
    );
  }

  // ── shapes it must NOT report as calls ───────────────────────────────────────────
  assert!(
    seen.len() == must.len(),
    "CACHE_CALL_CENSUS reported {} call(s), expected exactly {} — a function item, an identifier \
     that merely starts with a guarded name, and a comment must none of them count as calls.\nsaw: {seen:?}",
    seen.len(),
    must.len()
  );
  // The function item is not a call — and is surfaced separately rather than ignored, because a
  // later call through that pointer would be invisible. Raw or not: the second entry is the same
  // shape spelled `Cache::r#peek_one`, which matched nothing before the unrawing went in.
  assert!(
    found.function_items
      == std::vec![
        (std::string::String::from("function_item"), 39),
        (std::string::String::from("raw_function_item"), 64),
      ],
    "CACHE_CALL_CENSUS must report a guarded method NAMED as a function item, and must not count \
     it as a call: {:?}",
    found.function_items
  );
  // An anchor with nothing bound to it is dead documentation.
  assert!(
    found.orphan_anchors == std::vec![47],
    "CACHE_CALL_CENSUS misses an ORPHAN anchor, so a marker nobody maintains would read like \
     coverage: {:?}",
    found.orphan_anchors
  );
  // A macro that is not on the list is refused rather than read. `cache_call!(cache)` is the R8
  // falsification exactly: nothing guarded in its tokens, a body that parses as one clean
  // expression, and an expansion that can be `cache.pop_front()`. Walking it harder finds
  // nothing; the only sound answer is to refuse it.
  assert!(
    found.unknown_macros
      == std::vec![(
        std::string::String::from("parseable_macro"),
        std::string::String::from("cache_call"),
        68,
      )],
    "CACHE_CALL_CENSUS must refuse a macro whose expansion it cannot account for, however cleanly \
     the invocation parses: {:?}",
    found.unknown_macros
  );
  // An attribute macro rewrites the item being read, and never reaches `visit_macro` at all. A
  // third-party derive is the same hole behind a built-in attribute, which is why `derive`'s
  // arguments are checked on their own.
  assert!(
    found.unknown_attrs
      == std::vec![
        (
          std::string::String::from("attributed"),
          std::string::String::from("some_attribute_macro"),
          71,
        ),
        (
          std::string::String::from("third_party_derive"),
          std::string::String::from("derive(ThirdPartyDerive)"),
          75,
        ),
      ],
    "CACHE_CALL_CENSUS must refuse an attribute it cannot account for, including one hiding in a \
     `derive` list: {:?}",
    found.unknown_attrs
  );
  // Nothing here is an allowlisted macro with a body the parse cannot read.
  assert!(
    found.opaque_macros.is_empty(),
    "CACHE_CALL_CENSUS reported an opaque macro where there is none: {:?}",
    found.opaque_macros
  );
}

/// CACHE_CALL_CENSUS — the residual bound, made visible rather than assumed away.
///
/// After the allowlist there is exactly one syntax-adjacent hole left, and it is much smaller than
/// the one the substring era claimed: an **allowlisted** macro whose body does not parse as an
/// expression list. The expansion is accounted for — that is what admission to
/// [`MACRO_ALLOWLIST`] means — but the body was not walked, so a call written inside it went
/// unseen. Both halves are planted here, including the raw-identifier form, which is the quietest
/// of all: unread tokens, and a spelling that matched nothing until #183's eighth round.
///
/// A macro that is **not** allowlisted is a different answer, not a smaller one: it is refused
/// outright and its tokens are not read, because the tokens of a macro whose meaning is unknown
/// are not evidence in either direction. `macro_rules` is not on the list, so defining a macro in
/// the censused file is refused by the same rule — which is also why a macro defined *elsewhere*
/// in the crate is covered: only its invocation is in this file, and the invocation is what the
/// list governs.
#[test]
#[cfg_attr(
  miri,
  ignore = "parses a literal and walks it: no UB surface, and miri interprets every byte"
)]
fn cache_call_census_reports_a_macro_body_it_cannot_read() {
  // `matches!` is allowlisted and its expansion introduces no call — but a guard clause is not an
  // expression, so `parse_terminated` stops and the body goes unwalked. A realistic shape, not a
  // contrived one.
  let planted = r#"
impl Thing {
  fn hides_a_call(&self) {
    matches!(x, Some(_) if y.pop_front().is_some())
  }

  fn hides_a_raw_call(&self) {
    matches!(x, Some(_) if y.r#pop_back().is_some())
  }

  fn hides_nothing(&self) {
    matches!(x, Some(_) if y.len() > 0)
  }

  fn defines_a_macro(&self) {
    macro_rules! sneaky { ($c:expr) => { $c.pop_front() }; }
  }

  fn calls_it(&self, cache: &mut C) {
    let _ = sneaky!(cache);
  }
}
"#;
  let found = census_scan(planted);
  assert!(
    found.opaque_macros
      == std::vec![
        OpaqueMacro {
          function: std::string::String::from("hides_a_call"),
          path: std::string::String::from("matches"),
          line: 4,
          names_guarded: true,
        },
        OpaqueMacro {
          function: std::string::String::from("hides_a_raw_call"),
          path: std::string::String::from("matches"),
          line: 8,
          names_guarded: true,
        },
        OpaqueMacro {
          function: std::string::String::from("hides_nothing"),
          path: std::string::String::from("matches"),
          line: 12,
          names_guarded: false,
        },
      ],
    "CACHE_CALL_CENSUS must report every allowlisted macro body it could not walk — the raw \
     spelling included, and the harmless one too, because an unread region is what it must never \
     pass over quietly: {:?}",
    found.opaque_macros
  );
  // Defining a macro in the censused file, and invoking it: both refused by MACRO_ALLOWLIST, and
  // this is the pair that made the R8 bound false. The definition may live anywhere in the crate;
  // the invocation is what has to be here, and the invocation is what is checked.
  assert!(
    found.unknown_macros
      == std::vec![
        (
          std::string::String::from("defines_a_macro"),
          std::string::String::from("macro_rules"),
          16,
        ),
        (
          std::string::String::from("calls_it"),
          std::string::String::from("sneaky"),
          20,
        ),
      ],
    "CACHE_CALL_CENSUS must refuse a macro definition in the censused file AND any invocation of \
     an unlisted macro: {:?}",
    found.unknown_macros
  );
  assert!(
    found.calls.is_empty(),
    "a body that does not parse, and a macro that is refused unread, both yield no calls — by \
     construction: {:?}",
    found.calls
  );
}

/// CACHE_CALL_CENSUS — the seven drift arms, each driven at least once.
///
/// A census is two halves: a walk that finds things, and a report that names them. The other
/// controls exercise the walk by asserting on what [`census_scan`] returns. Nothing exercised the
/// **report** — on a conforming `cache.rs` not one `drift` arm runs, so every failure message
/// here was unexecuted code, and a swapped placeholder in one would be invisible in exactly the
/// situation the message exists for.
///
/// So this plants one source that trips all seven at once and asserts the exact lines. The
/// planted file is also the permanent form of the two falsifications #183's eighth round left
/// open: `r#pop_front` for the raw-identifier finding, `hidden!` for the macro-expansion one.
/// Both were reproduced by hand against the recovered census — green, with the calls live — and a
/// reproduction that is not a test is a story, so they are tests.
#[test]
#[cfg_attr(
  miri,
  ignore = "parses a literal and formats strings: no UB surface, and miri interprets every byte"
)]
fn cache_call_census_names_every_defect_it_reports() {
  // `drained_front` and `push_back_expecting` are real CALL_SITES rows, used here so the
  // registered-count and anchor-kind arms have something to disagree with.
  let planted = r#"
impl Thing {
  fn drained_front(&self, cache: &mut C) {
    let _ = cache.pop_front();
    // CACHE_CALL_CENSUS: absence
    let _ = cache.r#pop_front();
  }

  fn push_back_expecting(&self, cache: &mut C) {
    // CACHE_CALL_CENSUS: routed
    let _ = other.push_back(tok);
    let _ = Cache::peek_one;
    let _ = hidden!(cache);
    matches!(x, Some(_) if y.len() > 0)
  }

  #[rewrites_me]
  fn attributed(&self) {
    // CACHE_CALL_CENSUS: routed
    let _ = 1;
  }
}
"#;
  let drift = census_drift(&census_scan(planted));
  let want = std::vec![
    // 1a. A call with no anchor of its own — the plain, ordinary miss, and the one the whole
    //     census exists for.
    std::string::String::from(
      "conformance/cache.rs:4: `drained_front` calls `cache.pop_front` with no `// CACHE_CALL_CENSUS: <kind>` anchor of its own"
    ),
    // 1b. Anchored `absence` where CALL_SITES registers `routed`: a site reclassified by editing
    //     one of the two places. Also the RAW-IDENTIFIER finding — `r#pop_front` is seen at all
    //     only because the walk unraws, and before that it matched nothing anywhere.
    std::string::String::from(
      "conformance/cache.rs:6: `drained_front`'s `cache.pop_front` is anchored `absence` but registered `routed`"
    ),
    // 1c. Anchored and registered, but not for THIS receiver.
    std::string::String::from(
      "conformance/cache.rs:11: `push_back_expecting` calls `other.push_back` (anchored `routed`) and is NOT in CALL_SITES for that receiver"
    ),
    // 2. The inventory, in both directions.
    std::string::String::from(
      "CACHE_CALL_CENSUS: `drained_front` calls `cache.pop_front` 2 time(s), registered for 1"
    ),
    // 3. An anchor with no call bound to it.
    std::string::String::from(
      "conformance/cache.rs:19: a `CACHE_CALL_CENSUS:` anchor with no call bound to it"
    ),
    // 4. A guarded method named as a function item.
    std::string::String::from(
      "conformance/cache.rs:12: `push_back_expecting` names a guarded method as a function item rather than calling it; a call through that pointer would be invisible to this census"
    ),
    // 5. The MACRO-EXPANSION finding: an invocation naming nothing guarded, which the census now
    //    refuses instead of walking.
    std::string::String::from(
      "conformance/cache.rs:13: `push_back_expecting` invokes `hidden!`, which is not in MACRO_ALLOWLIST; syn does not expand macros, so its expansion could contain a guarded call that appears nowhere in this source"
    ),
    // 6. An attribute macro, which never reaches `visit_macro` at all.
    std::string::String::from(
      "conformance/cache.rs:17: `attributed` carries `#[rewrites_me]`, which is not allowlisted; an attribute macro rewrites the item this census is reading"
    ),
    // 7. The residue: an allowlisted macro whose body the parse could not read.
    std::string::String::from(
      "conformance/cache.rs:14: `push_back_expecting`'s `matches!` body does not parse as an expression list, so the census did not walk it"
    ),
  ];
  let missing: std::vec::Vec<&std::string::String> =
    want.iter().filter(|w| !drift.contains(w)).collect();
  assert!(
    missing.is_empty(),
    "CACHE_CALL_CENSUS drift messages changed or stopped firing.\nmissing:\n  {}\nactual:\n  {}",
    missing
      .iter()
      .map(|s| s.as_str())
      .collect::<std::vec::Vec<_>>()
      .join("\n  "),
    drift.join("\n  ")
  );
  // Arm 2 sweeps the whole of CALL_SITES, so scanning a snippet also reports every row this
  // planted file does not contain — an artifact of the fixture, not of the census. So exactness
  // is checked over the other six arms, the ones driven by what the file *contains*: an extra
  // line among those means a shape is reported twice, or reported where it should not be.
  let (inventory, located): (std::vec::Vec<_>, std::vec::Vec<_>) = drift
    .iter()
    .partition(|d| d.starts_with("CACHE_CALL_CENSUS: "));
  let want_located = want.len() - 1;
  assert!(
    located.len() == want_located,
    "CACHE_CALL_CENSUS reported {} located drift line(s), expected exactly {}.\nactual:\n  {}",
    located.len(),
    want_located,
    drift.join("\n  ")
  );
  // And the inventory arm fires for the planted count mismatch specifically, not merely somewhere.
  assert!(
    inventory.iter().any(|d| d.as_str()
      == "CACHE_CALL_CENSUS: `drained_front` calls `cache.pop_front` 2 time(s), registered for 1"),
    "the inventory arm must name the site whose count moved:\n  {}",
    drift.join("\n  ")
  );
  // And the arm that must stay silent: `names_guarded` is only appended when the unread tokens
  // really do name one, so the harmless `matches!` above must NOT carry that tail.
  assert!(
    !drift
      .iter()
      .any(|d| d.contains("and its tokens name a guarded method")),
    "the opaque-macro tail is for a body whose tokens name a guarded method; this one's do not:\n  {}",
    drift.join("\n  ")
  );
}

// ── The corpus builder's own anti-hang bound ────────────────────────────────────────
//
// `CacheHarness::corpus` fills until `out.len() < want` is false. That gate counts the tokens it
// KEPT while the loop consumes the whole item stream, and an `Err` grows neither `out` nor the
// lexer's exhaustion — so a lexer that only errors never reaches `want` and never returns `None`.
// Same shape as an item budget read at the `next()` boundary while `next()` loops over the errors
// it accepts: the counter measures a filtered subset of what the loop consumes.

/// A hand-rolled lexer that neither advances nor exhausts: every [`lex`](Lexer::lex) returns the
/// same nonempty error over the same span. Not logos-backed, deliberately — the defect is in the
/// kit's own loop, not in any adapter.
struct EndlessErrCorpusLexer<'a> {
  src: &'a str,
  state: CState,
}

impl<'a> Lexer<'a> for EndlessErrCorpusLexer<'a> {
  type State = CState;
  type Source = str;
  type Token = CTok<'a>;
  type Span = crate::SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      state: CState::default(),
    }
  }
  fn with_state(src: &'a str, state: CState) -> Self {
    Self { src, state }
  }
  fn check(&self) -> Result<(), CErr> {
    Ok(())
  }
  fn state(&self) -> &CState {
    &self.state
  }
  fn state_mut(&mut self) -> &mut CState {
    &mut self.state
  }
  fn into_state(self) -> CState {
    self.state
  }
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> crate::SimpleSpan {
    crate::SimpleSpan::new(0, self.src.len().min(1))
  }
  fn slice(&self) -> &'a str {
    &self.src[..self.src.len().min(1)]
  }
  fn lex(&mut self) -> Option<Result<CTok<'a>, CErr>> {
    if self.src.is_empty() {
      return None;
    }
    Some(Err(CErr::Any))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, _n: &usize) {}
}

#[test]
#[should_panic(expected = "lex-budget")]
fn a_corpus_of_nothing_but_errors_is_refused_not_spun_on() {
  // Before the attempt ceiling this did not fail — it never returned. The kit's own driver is the
  // subject here, so any cache serves; the run never reaches a check.
  CacheHarness::<EndlessErrCorpusLexer<'_>, DefaultCache<'_, EndlessErrCorpusLexer<'_>>>::new(
    "abc def",
  )
  .named("endless-error corpus")
  .run();
}

// ── The knob's boundary: three inputs, three separate refusals ──────────────────────
//
// This was one cell, and the one was vacuous: it passed `usize::MAX` and accepted any panic
// tagged `lex-budget`, so a clamp to a LOWER multiple than the maximum satisfied it just as well
// as the maximum did, and so did the release wrapped-to-zero counter. A boundary cell that cannot
// distinguish the boundary from its neighbours is asserting that some panic happened.
//
// The exact maximum, one above it, and `usize::MAX` are three cells now. The maximum is ACCEPTED
// and its guard fires with the corpus builder's own wording and its own number; the two above it
// never reach a lexer, because the builder refuses them by name.

/// The exact maximum is **accepted**, and the guard still fires under it.
///
/// One source unit, so the accepted ceiling is `65536 * 1 + 64 = 65600` attempts and the cell costs
/// milliseconds. It is the *setting* that is at its maximum here, not the ceiling — and the ceiling
/// is in the expectation, because a cap enforced one short of itself would print a smaller one.
#[test]
#[should_panic(
  expected = "lex-budget]: building the corpus asked the lexer to lex more than 65600 times"
)]
fn the_exact_maximum_lex_attempts_multiple_is_accepted_and_still_refuses_an_endless_lexer() {
  CacheHarness::<EndlessErrCorpusLexer<'_>, DefaultCache<'_, EndlessErrCorpusLexer<'_>>>::new("a")
    .named("endless-error corpus at the maximum multiple")
    .lex_attempts_multiple(super::MAX_BUDGET_MULTIPLE)
    .run();
}

/// One above the maximum is **refused at the knob**, not lowered to it.
///
/// No `run()`: the panic must come from the builder, so a return to clamping fails this with "did
/// not panic" instead of being satisfied by whatever the clamped run does next. The expectation
/// carries the knob's name, the supported maximum and the rejected value — none of which the
/// corpus builder's `lex-budget` message contains.
#[test]
#[should_panic(
  expected = "CacheHarness::lex_attempts_multiple is capped at 65536 attempts per source unit and was given 65537"
)]
fn one_above_the_maximum_lex_attempts_multiple_is_refused_at_the_knob() {
  let _ =
    CacheHarness::<EndlessErrCorpusLexer<'_>, DefaultCache<'_, EndlessErrCorpusLexer<'_>>>::new(
      "a",
    )
    .lex_attempts_multiple(super::MAX_BUDGET_MULTIPLE + 1);
}

/// `usize::MAX` — the historical disarm value — takes the same refusal.
///
/// Separate from the cell above because this is the value that used to be *accepted*: silently
/// clamped, run, and then refused with a `lex-budget` tag the caller reads as a verdict on their
/// lexer. The rejected value is left out of the expectation because its rendering is target-width
/// dependent; the knob's name and the maximum are in it, and no clamp and no `lex-budget` refusal
/// can produce those.
#[test]
#[should_panic(
  expected = "CacheHarness::lex_attempts_multiple is capped at 65536 attempts per source unit and was given "
)]
fn the_largest_usize_lex_attempts_multiple_is_refused_at_the_knob() {
  let _ =
    CacheHarness::<EndlessErrCorpusLexer<'_>, DefaultCache<'_, EndlessErrCorpusLexer<'_>>>::new(
      "a",
    )
    .lex_attempts_multiple(usize::MAX);
}

// ── Dense but finite: what the ceiling must not conflate with nontermination ─────────
//
// The guard above stops a lexer that never reaches the target. The ceiling it stops at was
// justified by "monotone progress and nonempty spans bound a conforming lexer at one item per
// source unit" — which is not what the contract enforced elsewhere says. Starts must be
// NON-DECREASING, not strictly increasing, and spans must be INDIVIDUALLY nonempty, not disjoint,
// so repeated and overlapping items are legal and a finite terminating lexer may emit many items
// per unit. This kit does not even check that much: it drives the raw lexer and never runs the
// lexer-contract tier.
//
// So the number was an assumption wearing a derivation's clothes, and a lexer that merely emits a
// lot per unit was refused as nonterminating with no way to say otherwise. The two cells below are
// the same lexer at the same density under the two ceilings.

/// Lexer errors this fixture emits at each position before the token there.
///
/// Chosen so `want` tokens cost more than the default ceiling over [`DENSE_SRC`] and comfortably
/// less than the raised one, which is what makes the pair of cells below say something.
const DENSE_ERRORS_PER_TOKEN: u32 = 24;

/// Eight ASCII units, so the corpus is long enough for `DefaultCache`'s residency plus its peek
/// window while the default ceiling stays small.
const DENSE_SRC: &str = "abcdefgh";

/// A **finite, deterministic, terminating** lexer that emits [`DENSE_ERRORS_PER_TOKEN`] errors at
/// a position before the token there, and exhausts at the end of the source like any other.
///
/// Every error reports the same nonempty span as the token that follows it — legal, since the
/// contract asks for non-decreasing starts rather than strictly increasing ones — and the token
/// carries its own slice, so the corpus stays pairwise distinct on all three axes and the kit's
/// comparisons keep their discriminating power.
struct DenseErrCorpusLexer<'a> {
  src: &'a str,
  at: usize,
  start: usize,
  end: usize,
  burst: u32,
  state: CState,
}

impl<'a> Lexer<'a> for DenseErrCorpusLexer<'a> {
  type State = CState;
  type Source = str;
  type Token = CTok<'a>;
  type Span = crate::SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      at: 0,
      start: 0,
      end: 0,
      burst: 0,
      state: CState::default(),
    }
  }
  fn with_state(src: &'a str, state: CState) -> Self {
    Self {
      src,
      at: 0,
      start: 0,
      end: 0,
      burst: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), CErr> {
    Ok(())
  }
  fn state(&self) -> &CState {
    &self.state
  }
  fn state_mut(&mut self) -> &mut CState {
    &mut self.state
  }
  fn into_state(self) -> CState {
    self.state
  }
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> crate::SimpleSpan {
    crate::SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<CTok<'a>, CErr>> {
    if self.at >= self.src.len() {
      return None;
    }
    self.start = self.at;
    self.end = self.at + 1;
    if self.burst < DENSE_ERRORS_PER_TOKEN {
      self.burst += 1;
      return Some(Err(CErr::Any));
    }
    self.burst = 0;
    self.at = self.end;
    self.state.lexed += 1;
    Some(Ok(CTok::Word(&self.src[self.start..self.end])))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.at += *n;
    self.start = self.at;
    self.end = self.at;
  }
}

/// The false failure the knob exists to resolve, pinned so it is a documented limit of the default
/// rather than a surprise — and so the refusal keeps naming the way out of it.
///
/// This lexer terminates. What the default ceiling refuses is its *density*.
#[test]
#[should_panic(expected = "raise the ceiling with CacheHarness::lex_attempts_multiple")]
fn the_default_ceiling_refuses_a_lexer_that_is_merely_dense() {
  CacheHarness::<DenseErrCorpusLexer<'_>, DefaultCache<'_, DenseErrCorpusLexer<'_>>>::new(
    DENSE_SRC,
  )
  .named("dense-error corpus")
  .run();
}

/// And the whole contract passes at that same density once the ceiling is raised: the guard bounds
/// nontermination, not slowness.
///
/// This is the half that matters. A guard that refuses a legitimate lexer with no override is a
/// certification that cannot be run — a safe direction to fail in, and still a failure.
#[test]
fn a_finite_dense_error_lexer_passes_once_the_ceiling_is_raised() {
  CacheHarness::<DenseErrCorpusLexer<'_>, DefaultCache<'_, DenseErrCorpusLexer<'_>>>::new(
    DENSE_SRC,
  )
  .named("dense-error corpus")
  .lex_attempts_multiple(64)
  .run();
}
