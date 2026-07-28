//! Tests that prove the cache conformance kit: the four built-in caches pass every check, a
//! correct third-party queue passes too, and each deliberately-broken queue trips exactly the
//! check that owns its defect.
//!
//! The broken fixtures are one `Queue` type with a const-selected defect, so the defects sit side
//! by side and adding one is three lines rather than a new file.

use core::cell::Cell;
use std::collections::VecDeque;

use mayber::Maybe;

use super::cache::CacheHarness;
use crate::{
  Lexer, Token,
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

/// The corpus every cell runs over: ten items, one past the widest capacity the cells use, so
/// every cache here fills and then refuses.
const SRC: &str = "a b c d e f g h i j";

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

/// Defect selectors for [`Queue`]. `SOUND` is the audit's deliberately-literal witness queue —
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

/// A `VecDeque`-backed third-party cache with a const-selected defect.
struct Queue<'a, L, const D: u8>
where
  L: Lexer<'a>,
{
  items: VecDeque<CachedTokenOf<'a, L>>,
  cap: usize,
  peeks: Cell<usize>,
}

impl<'a, L, Lang: ?Sized, const D: u8> Cache<'a, L, Lang> for Queue<'a, L, D>
where
  L: Lexer<'a>,
{
  type Options = usize;

  const RETAINS_FRONT: bool = D != APPENDING_PUSH_FRONT;

  fn new() -> Self {
    <Self as Cache<'a, L, Lang>>::with_options(4)
  }

  fn with_options(cap: usize) -> Self {
    Self {
      items: VecDeque::with_capacity(cap),
      cap,
      peeks: Cell::new(0),
    }
  }

  fn len(&self) -> usize {
    if D == SHORT_LEN {
      return self.items.len().saturating_sub(1);
    }
    self.items.len()
  }

  fn remaining(&self) -> usize {
    self.cap - self.items.len()
  }

  fn push_front(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
    if D == LYING_RETAINS_FRONT || self.items.len() == self.cap {
      return Err(self.refuse(tok));
    }
    if D == APPENDING_PUSH_FRONT {
      self.items.push_back(tok);
      return Ok(self.items.back().expect("just pushed").as_ref());
    }
    self.items.push_front(tok);
    Ok(self.items.front().expect("just pushed").as_ref())
  }

  fn push_back(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
    if self.items.len() == self.cap {
      return Err(self.refuse(tok));
    }
    self.items.push_back(tok);
    Ok(self.items.back().expect("just pushed").as_ref())
  }

  fn pop_front(&mut self) -> Option<CachedTokenOf<'a, L>> {
    if D == LIFO_POP_FRONT {
      return self.items.pop_back();
    }
    self.items.pop_front()
  }

  fn pop_back(&mut self) -> Option<CachedTokenOf<'a, L>> {
    if D == FIFO_POP_BACK {
      return self.items.pop_front();
    }
    self.items.pop_back()
  }

  fn clear(&mut self) {
    self.items.clear();
  }

  fn peek<'p, W>(
    &'p self,
    buf: &mut generic_arraydeque::GenericArrayDeque<MaybeRefCachedTokenOf<'p, 'a, L>, W::CAPACITY>,
  ) where
    W: crate::Window,
  {
    let seen = self.peeks.get();
    self.peeks.set(seen + 1);
    let mut fill = buf.remaining_capacity().min(self.items.len());
    if D == IMPURE_PEEK && seen > 0 {
      fill = fill.saturating_sub(1);
    }
    if D == REVERSED_PEEK {
      for tok in self.items.iter().rev().take(fill) {
        buf.push_back(Maybe::Ref(tok.as_ref()));
      }
      return;
    }
    for tok in self.items.iter().take(fill) {
      buf.push_back(Maybe::Ref(tok.as_ref()));
    }
  }

  fn front(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
    self.items.front().map(CachedToken::as_ref)
  }

  fn back(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
    self.items.back().map(CachedToken::as_ref)
  }
}

impl<'a, L, const D: u8> Queue<'a, L, D>
where
  L: Lexer<'a>,
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
