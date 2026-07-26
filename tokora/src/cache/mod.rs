use ::generic_arraydeque::{GenericArrayDeque, typenum::U1};
use mayber::Maybe;

use crate::{
  Window,
  input::Checkpoint,
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
/// # Design Patterns
///
/// Different implementations support different use cases:
/// - **Fixed-size arrays**: Bounded lookahead with known maximum (e.g., `[CachedToken; 4]`)
/// - **Dynamic buffers**: Unlimited lookahead using `Vec` or `VecDeque`
/// - **BlackHole**: No caching at all, for streaming-only scenarios without backtracking
///
/// Note: Tokens cannot be overwritten until explicitly consumed, as they must remain
/// available for backtracking operations. This means the cache can become full and
/// refuse new tokens if capacity is reached.
///
/// # Cache Operations
///
/// The cache supports standard queue operations:
/// - `push_back`: Add newly lexed tokens to the end (fails if cache is full)
/// - `pop_front`: Remove and return the oldest token
/// - `peek`: View tokens without removing them
/// - `rewind`: Restore to a previous state (for backtracking)
///
/// # Safety
///
/// The `peek` method is marked unsafe because it requires implementations to guarantee
/// that returned slices only contain properly initialized tokens. This is enforced by
/// the trait's contract.
///
/// # Example
///
/// ```ignore
/// // A simple fixed-size cache using a VecDeque-like structure
/// struct BoundedCache<'a, T: Token<'a>> {
///     tokens: VecDeque<CachedToken<'a, T>>,
///     capacity: usize,
/// }
///
/// impl<'a, T: Token<'a>> Cache<'a, T> for BoundedCache<'a, T> {
///     fn len(&self) -> usize {
///         self.tokens.len()
///     }
///
///     fn remaining(&self) -> usize {
///         self.capacity - self.tokens.len()
///     }
///
///     fn push_back(&mut self, tok: CachedToken<'a, T>) -> Result<&CachedToken<'a, T>, CachedToken<'a, T>> {
///         if self.tokens.len() < self.capacity {
///             self.tokens.push_back(tok);
///             Ok(self.tokens.back().unwrap())
///         } else {
///             Err(tok) // Cache full, cannot overwrite!
///         }
///     }
///     // ... other methods
/// }
/// ```
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
  fn len(&self) -> usize;

  /// Returns the number of additional tokens that can be cached.
  ///
  /// For unbounded caches (like `Vec`), this might return a large number.
  /// For fixed-size caches, this returns the number of free slots.
  /// For black hole caches, this always returns 0.
  fn remaining(&self) -> usize;

  /// Rewinds the cache to a previously saved checkpoint.
  ///
  /// This operation restores the cache state to match the checkpoint, typically
  /// by clearing any tokens that were added after the checkpoint was created.
  /// This is used for parser backtracking.
  fn rewind(&mut self, checkpoint: &Checkpoint<'a, '_, L>)
  where
    Self: Sized,
    L: Lexer<'a>;

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
  ///     Ok(cached_ref) => {
  ///         // Token was cached successfully
  ///     }
  ///     Err(token) => {
  ///         // Cache is full, handle token directly
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

  /// Removes and returns the token at the back of the cache.
  ///
  /// Returns `None` if the cache is empty. This is less commonly used than
  /// `pop_front` but can be useful for certain cache management operations.
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

  /// Peeks at multiple cached tokens without removing them.
  ///
  /// Fills the provided buffer with references to cached tokens (or owned tokens if
  /// necessary). The returned slice contains only the successfully initialized tokens,
  /// which may be fewer than requested if the cache doesn't have enough tokens.
  ///
  /// # Parameters
  ///
  /// - `buf`: A buffer of uninitialized `MaybeRef` entries to fill with peeked tokens
  ///
  /// # Returns
  ///
  /// A mutable slice containing initialized token references. The slice length indicates
  /// how many tokens were actually available.
  ///
  /// # Safety
  ///
  /// Implementations must guarantee that:
  /// - The returned slice contains only properly initialized tokens
  /// - All cached tokens are filled into the buffer if the buffer is large enough
  /// - The slice bounds are correct and don't include uninitialized memory
  ///
  /// Callers must ensure the returned slice is not used beyond its lifetime.
  #[allow(clippy::mut_from_ref)]
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
