use ::generic_arraydeque::{GenericArrayDeque, typenum::U1};
use mayber::Maybe;

use crate::{
  Window,
  lexer::Lexer,
  span::{Span, Spanned},
};

mod blackhole;
mod generic_arraydeque;
mod option;

/// A peeked buffer of tokens from the lexer.
pub type Peeked<'p, 'inp, L, W> = ::generic_arraydeque::GenericArrayDeque<
  MaybeRefCachedTokenOf<'p, 'inp, L>,
  <W as Window>::CAPACITY,
>;

/// A peeked buffer of tokens from the lexer.
pub type PeekedToken<'p, 'inp, L, W> = ::generic_arraydeque::GenericArrayDeque<
  MaybeRefCachedTokenOf<'p, 'inp, L, <L as Lexer<'inp>>::Token, <L as Lexer<'inp>>::Span>,
  <W as Window>::CAPACITY,
>;

/// The default cache type used by the lexer.
pub type DefaultCache<'a, L> =
  ::generic_arraydeque::GenericArrayDeque<CachedTokenOf<'a, L>, ::generic_arraydeque::typenum::U3>;

/// A trait for caching lookahead tokens in the tokenizer.
///
/// `Cache` provides a buffer for tokens that have been lexed but not yet consumed,
/// enabling efficient lookahead and backtracking operations. The cache acts as a
/// queue (FIFO - First In, First Out) between the lexer and the parser.
///
/// # Purpose
///
/// The cache serves several critical functions:
/// - **Lookahead**: Allows peeking at future tokens without consuming them
/// - **Backtracking**: Supports parser backtracking via checkpoint/rewind operations
/// - **Efficiency**: Avoids re-lexing tokens that have already been processed
/// - **State Management**: Preserves lexer state (extras) alongside each token
///
/// # The implementations, and the limit
///
/// The complete inventory is three, all bounded at compile time:
///
/// - `GenericArrayDeque<_, N>` — a fixed-capacity ring, `N` a `typenum` bound.
///   [`DefaultCache`] is this at `U3`.
/// - `Option<CachedToken<..>>` — capacity 1, the smallest cache that can still retain the front.
/// - `()` — capacity 0: no caching at all, for streaming-only use without lookahead.
///
/// There is **no dynamic, allocator-backed cache**, and enabling `alloc` does not add one — it
/// enables allocator-backed *containers and drivers*, and forwards the sub-crates' own `alloc`
/// features. Nor would one buy unbounded lookahead: [`crate::Window`] is sealed at
/// **U1–U32**, so no cache of any capacity — built-in or downstream — can serve a public peek
/// past 32 tokens. Lookahead beyond the window is what **transactions** are for: an
/// [`attempt`](crate::InputRef::attempt) or a [`Transaction`](crate::Transaction) speculates
/// arbitrarily far and re-lexes on rollback, which is unbounded speculation without an unbounded
/// buffer.
///
/// Note: Tokens cannot be overwritten until explicitly consumed, as they must remain
/// available for backtracking operations. This means the cache can become full and
/// refuse new tokens if capacity is reached.
///
/// # The contract: a cache is a queue, and only a queue
///
/// The input layer owns every decision about *which* tokens should be resident; a cache decides
/// only whether it has room. These are the laws it must uphold, phrased over what a caller can
/// observe — the cache conformance kit checks each one:
///
/// - **FIFO append.** [`push_back`](Cache::push_back) places the token after every resident
///   entry; [`push_front`](Cache::push_front) places it before every resident entry. A refused
///   push returns the token **unchanged** and leaves the cache exactly as it was.
/// - **Order.** [`pop_front`](Cache::pop_front) removes the **oldest** resident entry and
///   [`pop_back`](Cache::pop_back) the **newest**; [`front`](Cache::front) and
///   [`back`](Cache::back) view those same two entries without removing them.
/// - **Exact length.** [`len`](Cache::len) is exactly the resident count, and
///   [`remaining`](Cache::remaining) is exactly how many more `push_back`s will be accepted.
/// - **Bounded, pure peek.** [`peek`](Cache::peek) appends exactly `min(len(), buffer capacity)`
///   entries to the buffer, oldest first, once each. It is logically pure: it takes `&self`,
///   calling it twice yields the same sequence, and it changes no observable. Borrowed and owned
///   results denote the same tokens.
/// - **The restore path must not panic.** [`pop_front`](Cache::pop_front),
///   [`pop_back`](Cache::pop_back), [`clear`](Cache::clear), [`front`](Cache::front),
///   [`front_span`](Cache::front_span), [`len`](Cache::len) — **and
///   [`push_front`](Cache::push_front)** — are called by the input layer from guard drops, which
///   can run **mid-unwind**, where a panic aborts the process. `push_front` is on that list for a
///   reason worth stating: a scan's unwind edge puts the in-flight token back through it, so it
///   is a restore-path operation even though it is the only *writing* one. A `push_front` that
///   panics on the normal path is also the one place the crate cannot make whole — it has taken
///   the token by value, so the token goes with it.
/// - [`RETAINS_FRONT`](Cache::RETAINS_FRONT) carries its own law, and it is checked: declaring
///   `true` and then refusing a front push into an empty cache panics at the refusal site.
///
/// What is deliberately **not** on this list is any notion of a checkpoint. A cache never sees
/// one. The cursor-keyed geometry a restore needs is performed by the input layer through the
/// queue surface above, beside the push-generation bookkeeping only it can do. (`Cache::rewind`
/// used to ask a cache to do the first half from facts it was never given; see
/// [`InputRef::restore`](crate::InputRef::restore) and the 0.8.0 changelog.)
///
/// Also not on it: **which** of these operations an input path performs. The laws above fix what
/// each operation *means*; the choice between two calls that mean the same thing belongs to the
/// input layer and may change between versions. A head served by [`front`](Cache::front) and the
/// same head served by [`len`](Cache::len) + [`peek`](Cache::peek) are the same read, and
/// [`InputRef::peek_head_map`](crate::InputRef::peek_head_map) makes the first call where its
/// general route makes the other two. A conforming cache cannot tell them apart — every one of
/// those is a `&self` read that changes no observable — so a cache that *counts* its calls is
/// measuring the input layer, not the token stream.
///
/// The positive form of that, and the one to implement against: **make every method inert** — let
/// it do only what its own law above says it does, and always return normally. An inert cache
/// cannot tell one contract-equal choice of operations from another, so every such choice is free.
/// That is the same rule [`skip_while`](crate::InputRef::skip_while) and
/// [`peek_head_map`](crate::InputRef::peek_head_map) state as a condition on the caller, seen from
/// the end that implements it.
///
/// **Inert is the whole requirement**; what follows is not the set of ways to miss it. Two that
/// come up, and what each costs:
///
/// * **it counts.** Harmless while the count stays inside the cache. It stops being harmless the
///   moment the count reaches a **predicate or closure the same caller supplies** — a `skip_while`
///   predicate, a `peek_head_map` `f` — because then a measurement of the input layer becomes a
///   parse decision, and which operations ran decides which tokens are consumed;
/// * **it unwinds.** A method with a reachable panic path turns *which operations an input path
///   chose* into control flow. `skip_while` over a resident head that has nothing to skip reads
///   the head with [`front`](Cache::front) where the scan it replaces would have called
///   [`pop_front`](Cache::pop_front) and [`push_front`](Cache::push_front); if either of those can
///   panic, one route unwinds and the other returns `Ok(())`. The value guarantees on those two
///   methods are not made to a caller whose cache can do that.
///
/// Both are the same rule written on the trait that would be doing it, and the rule is the
/// sentence above them rather than the two bullets: a third way of losing inertness would cost the
/// same, and would need no amendment here.
pub trait Cache<'a, L, Lang: ?Sized = ()>: 'a {
  /// The options for creating a new cache.
  type Options;

  /// Whether a [`push_front`](Cache::push_front) into an **empty** cache always succeeds —
  /// i.e. the cache can retain at least one token.
  ///
  /// When `true`, the input layer's parked-front machinery (the retained home for a put-back a
  /// cache refuses) is statically unreachable and compiles out of every hot path. A cache that
  /// declares `true` and then refuses a front push into an empty cache is a contract violation
  /// and panics at the refusal site rather than losing the token.
  ///
  /// Conservative default: `false` (the parked slot stays live; only performance is affected).
  const RETAINS_FRONT: bool = false;

  /// Creates a new, empty cache.
  fn new() -> Self
  where
    Self: Sized;

  /// Creates a new, empty cache with the specified capacity.
  fn with_options(options: Self::Options) -> Self
  where
    Self: Sized;

  /// Returns `true` if the cache contains no tokens.
  ///
  /// This is a convenience method that checks if `len() == 0`.
  #[inline(always)]
  fn is_empty(&self) -> bool {
    self.len() == 0
  }

  /// Returns the number of tokens currently stored in the cache.
  ///
  /// This count includes all cached tokens from front to back.
  ///
  /// # It must be **exact**, and the peek fill is why
  ///
  /// [`InputRef::peek`](crate::input::InputRef::peek) sizes the window's cache region from
  /// this number *before* it lexes: it reserves one window slot per reported resident, spends
  /// the rest of the window on tokens lexed past the cache, and only then calls
  /// [`peek`](Cache::peek) into the room that is left. A `len` that is not the resident count
  /// therefore mis-sizes that copy — under-report and it is clipped mid-run with the
  /// later-lexed tokens closing the gap behind it, over-report and the window comes back short
  /// while the input still has more. The fill checks the copy it gets and **panics** rather
  /// than hand back a window that is wrong about the stream; see that method's `# Panics`.
  ///
  /// An upper bound, a capacity, or a count that lags a push is not a `len`.
  fn len(&self) -> usize;

  /// Returns the number of additional tokens that can be cached.
  ///
  /// For fixed-size caches, this returns the number of free slots.
  /// For black hole caches, this always returns 0.
  fn remaining(&self) -> usize;

  /// Attempts to add a token to the front of the cache.
  ///
  /// If successful, returns `Ok` with a reference to the cached token.
  /// If the cache is full, returns `Err` with the token so the caller can handle it
  /// (e.g., by processing it immediately without caching).
  ///
  /// # Example
  ///
  /// ```ignore
  /// match cache.push_front(token) {
  ///     Ok(cached) => {
  ///         // `cached: CachedTokenRefOf<'_, 'a, L>` — the entry, borrowed
  ///     }
  ///     Err(token) => {
  ///         // Cache full, and the token comes back UNCHANGED: handle it directly
  ///     }
  /// }
  /// ```
  fn push_front(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>>
  where
    L: Lexer<'a>;

  /// Attempts to add a token to the back of the cache.
  ///
  /// If successful, returns `Ok` with a reference to the cached token.
  /// If the cache is full, returns `Err` with the token so the caller can handle it
  /// (e.g., by processing it immediately without caching).
  ///
  /// # Example
  ///
  /// ```ignore
  /// match cache.push_back(token) {
  ///     Ok(cached_ref) => {
  ///         // Token was cached successfully
  ///     }
  ///     Err(token) => {
  ///         // Cache is full, handle token directly
  ///     }
  /// }
  /// ```
  fn push_back(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>>
  where
    L: Lexer<'a>;

  /// Removes and returns the token at the front of the cache.
  ///
  /// Returns `None` if the cache is empty. This is the primary way to consume
  /// cached tokens.
  #[allow(clippy::type_complexity)]
  fn pop_front(&mut self) -> Option<CachedTokenOf<'a, L>>
  where
    L: Lexer<'a>;

  /// Removes and returns the **newest** resident token — the one a `push_back` most recently
  /// accepted, or the last survivor of the resident run.
  ///
  /// Returns `None` if the cache is empty. The input layer's restore path is built on exactly
  /// this law: pushes append to the back only, so the entries lexed on an abandoned
  /// continuation are the newest ones, and dropping them is a run of `pop_back`s. It must not
  /// panic — a restore can run from a guard drop mid-unwind.
  #[allow(clippy::type_complexity)]
  fn pop_back(&mut self) -> Option<CachedTokenOf<'a, L>>
  where
    L: Lexer<'a>;

  /// Removes all tokens from the cache.
  ///
  /// After calling this method, `len()` returns 0 and `is_empty()` returns `true`.
  fn clear(&mut self);

  /// Conditionally removes and returns the front token if it matches a predicate.
  ///
  /// Peeks at the first token in the cache and checks if it satisfies the predicate.
  /// If it does, removes and returns it. Otherwise, returns `None` without modifying
  /// the cache.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Pop token only if it's a specific type
  /// if let Some(token) = cache.pop_front_if(|t| matches!(t.token().data, Lexed::Token(_))) {
  ///     // Process valid token
  /// }
  /// ```
  #[allow(clippy::type_complexity)]
  fn pop_front_if<F>(&mut self, predicate: F) -> Option<CachedTokenOf<'a, L>>
  where
    F: FnOnce(CachedTokenRefOf<'_, 'a, L>) -> bool,
    L: Lexer<'a>,
  {
    if let Some(peeked) = self.front() {
      if predicate(peeked) {
        return self.pop_front();
      }
    }
    None
  }

  /// Conditionally removes and returns the front token if it matches a validation predicate.
  ///
  /// Peeks at the first token in the cache and checks if it satisfies the predicate.
  /// If it does, removes and returns it. Otherwise, returns `None` without modifying
  /// the cache.
  #[allow(clippy::type_complexity)]
  fn try_pop_front_if<E, F>(&mut self, predicate: F) -> Option<Result<CachedTokenOf<'a, L>, E>>
  where
    F: FnOnce(CachedTokenRefOf<'_, 'a, L>) -> Result<(), E>,
    L: Lexer<'a>,
  {
    if let Some(peeked) = self.front() {
      return match predicate(peeked) {
        Ok(()) => self.pop_front().map(Ok),
        Err(e) => Some(Err(e)),
      };
    }
    None
  }

  /// Peeks at the first cached token without removing it.
  ///
  /// Returns `Some(MaybeRef)` with either a reference to the cached token or
  /// an owned token (if cache implementation requires). Returns `None` if the
  /// cache is empty.
  ///
  /// This is a convenience wrapper around `peek` for looking at just one token.
  #[inline(always)]
  fn peek_one<'c>(&self) -> Option<MaybeRefCachedTokenOf<'_, 'a, L>>
  where
    'a: 'c,
    L: Lexer<'a>,
  {
    let mut buf = GenericArrayDeque::new();
    self.peek::<U1>(&mut buf);
    buf.pop_front()
  }

  /// Peeks at multiple cached tokens without removing them, appending them to `buf`.
  ///
  /// # The law
  ///
  /// Append **exactly `min(len(), buf`'s remaining capacity`)`** entries, oldest first, each
  /// resident token once. The call is logically **pure**: it takes `&self`, it changes no
  /// observable of the cache, and calling it twice on an unchanged cache appends the same
  /// sequence both times. A borrowed entry and an owned one denote the same token, so a caller
  /// cannot tell which a cache chose to hand back.
  ///
  /// Given room for `len()` entries this must therefore deliver the resident run **entire**,
  /// [`front`](Cache::front) through [`back`](Cache::back). The one place a copy may stop short
  /// of `back` is where the buffer's own capacity stopped it. The peek fill leaves exactly the
  /// room [`len`](Cache::len) claimed and checks what comes back — see that method for the
  /// consequences of the two directions a wrong `len` can take.
  ///
  /// # Parameters
  ///
  /// - `buf`: the destination deque, appended to (not overwritten); its capacity is the bound.
  fn peek<'p, W>(
    &'p self,
    buf: &mut GenericArrayDeque<MaybeRefCachedTokenOf<'p, 'a, L>, W::CAPACITY>,
  ) where
    W: Window,
    L: Lexer<'a>;

  /// Pushes multiple tokens into the cache at once.
  ///
  /// Attempts to cache all tokens from the iterator. If the cache becomes full,
  /// returns an iterator over the tokens that could not be cached.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let overflow = cache.push_many(token_iter);
  /// for token in overflow {
  ///     // Handle tokens that didn't fit in cache
  /// }
  /// ```
  #[inline(always)]
  fn push_many<'p>(
    &'p mut self,
    toks: impl Iterator<Item = CachedTokenOf<'a, L>> + 'p,
  ) -> impl Iterator<Item = CachedTokenOf<'a, L>> + 'p
  where
    L: Lexer<'a>,
  {
    toks.filter_map(move |tok| self.push_back(tok).err())
  }

  /// Returns a reference to the front (oldest) cached token.
  ///
  /// Returns `None` if the cache is empty. This does not remove the token.
  fn front(&self) -> Option<CachedTokenRefOf<'_, 'a, L>>
  where
    L: Lexer<'a>;

  /// Returns a reference to the back (newest) cached token.
  ///
  /// Returns `None` if the cache is empty. This does not remove the token.
  fn back(&self) -> Option<CachedTokenRefOf<'_, 'a, L>>
  where
    L: Lexer<'a>;

  /// Returns the combined span covering all cached tokens.
  ///
  /// If the cache has tokens, returns a span from the start of the first token
  /// to the end of the last token. Returns `None` if the cache is empty.
  ///
  /// This is useful for error reporting or understanding the range of lookahead.
  #[inline(always)]
  fn span(&self) -> Option<L::Span>
  where
    L: Lexer<'a>,
  {
    match (self.front(), self.back()) {
      (Some(first), Some(last)) => Some(L::Span::new(
        first.token().span_ref().start(),
        last.token().span_ref().end(),
      )),
      _ => None,
    }
  }

  /// Returns the span of the first cached token.
  ///
  /// Returns `None` if the cache is empty. This is often used to determine
  /// where the next consumed token will come from.
  #[inline(always)]
  fn front_span<'s>(&'s self) -> Option<&'s L::Span>
  where
    'a: 's,
    L: Lexer<'a>,
  {
    self.front().map(move |t| *t.token().span())
  }

  /// Returns the span of the last cached token.
  ///
  /// Returns `None` if the cache is empty. This can be used to determine
  /// where the cache's lookahead ends.
  #[inline(always)]
  fn back_span<'s>(&'s self) -> Option<&'s L::Span>
  where
    'a: 's,
    L: Lexer<'a>,
  {
    self.back().map(move |t| *t.token().span())
  }
}

/// A cached token with its associated state for a specific lexer.
pub type CachedTokenOf<'a, L, T = <L as Lexer<'a>>::Token, Span = <L as Lexer<'a>>::Span> =
  CachedToken<T, <L as Lexer<'a>>::State, Span>;
/// A cached token with its associated state for a specific lexer.
pub type CachedTokenRefOf<'r, 'a, L, T = <L as Lexer<'a>>::Token, Span = <L as Lexer<'a>>::Span> =
  CachedToken<&'r T, &'r <L as Lexer<'a>>::State, &'r Span>;
/// A maybe reference to a cached token with its associated state for a specific lexer.
pub type MaybeRefCachedTokenOf<
  'r,
  'a,
  L,
  T = <L as Lexer<'a>>::Token,
  Span = <L as Lexer<'a>>::Span,
> = Maybe<CachedTokenRefOf<'r, 'a, L, T, Span>, CachedTokenOf<'a, L, T, Span>>;

/// Uniform access to a peeked token, hiding the borrowed/owned split of
/// [`MaybeRefCachedTokenOf`].
///
/// A peeked token is a [`Maybe`] whose `Ref` arm borrows a token from the cache
/// and whose `Owned` arm carries a token lexed past the cache window (the
/// overflow case). Both arms wrap a [`CachedToken`]; these accessors reach the
/// token and its span without the caller matching on the arm.
pub trait PeekedTokenExt<T, Span> {
  /// Returns a reference to the peeked token, regardless of arm.
  fn token(&self) -> &T;

  /// Returns a reference to the peeked token's span, regardless of arm.
  fn span(&self) -> &Span;
}

impl<T, State, Span> PeekedTokenExt<T, Span>
  for Maybe<CachedToken<&T, &State, &Span>, CachedToken<T, State, Span>>
{
  #[inline(always)]
  fn token(&self) -> &T {
    match self {
      Maybe::Ref(cached) => cached.token.data,
      Maybe::Owned(cached) => &cached.token.data,
    }
  }

  #[inline(always)]
  fn span(&self) -> &Span {
    match self {
      Maybe::Ref(cached) => cached.token.span,
      Maybe::Owned(cached) => &cached.token.span,
    }
  }
}

/// A cached token with its associated state.
pub struct CachedToken<T, State, Span> {
  pub(crate) token: Spanned<T, Span>,
  pub(crate) state: State,
}

impl<T, State, Span> Clone for CachedToken<T, State, Span>
where
  State: Clone,
  Span: Clone,
  T: Clone,
{
  #[inline(always)]
  fn clone(&self) -> Self {
    Self {
      token: self.token.clone(),
      state: self.state.clone(),
    }
  }
}

impl<T, State, Span> CachedToken<T, State, Span> {
  /// Creates a new cached token.
  #[inline(always)]
  pub(crate) const fn new(token: Spanned<T, Span>, state: State) -> Self {
    Self { token, state }
  }

  /// Returns a reference to the token.
  #[inline(always)]
  pub const fn token(&self) -> Spanned<&T, &Span> {
    self.token.as_ref()
  }

  /// Consumes the cached token and returns the lexed token.
  #[inline(always)]
  pub fn into_token(self) -> Spanned<T, Span> {
    self.token
  }

  /// Returns a reference to the cached token.
  #[inline(always)]
  pub const fn as_ref(&self) -> CachedToken<&T, &State, &Span> {
    CachedToken {
      token: self.token.as_ref(),
      state: &self.state,
    }
  }

  /// Maps the token to a new type using the provided function.
  #[inline(always)]
  pub fn map_token<U, F>(self, f: F) -> CachedToken<U, State, Span>
  where
    F: FnOnce(T) -> U,
  {
    CachedToken {
      token: self.token.map_data(f),
      state: self.state,
    }
  }

  /// Returns a reference to the state.
  #[inline(always)]
  pub const fn state(&self) -> &State {
    &self.state
  }

  /// Consumes the cached token and returns the extras.
  #[inline(always)]
  #[allow(clippy::type_complexity)]
  pub fn into_components(self) -> (Spanned<T, Span>, State) {
    (self.token, self.state)
  }
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(feature = "std")]
mod tests;

#[cfg(test)]
#[allow(warnings)]
#[cfg(feature = "std")]
mod cache_trait_tests;
