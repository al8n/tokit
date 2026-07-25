use core::mem::{ManuallyDrop, MaybeUninit};

use generic_arraydeque::{ArrayLength, GenericArrayDeque, array::GenericArray};

use super::*;

/// Drop-safe staging buffer for peek tokens that overflow the cache window.
///
/// A peek that looks past the cache capacity must hold the overflow tokens
/// somewhere until the cache region is copied into the output buffer. Those
/// tokens are **owned** (`Maybe::Owned`), so a raw `MaybeUninit` array would leak
/// them if an early return (a fatal lexer error emitted mid-scan) skipped the
/// hand-off. `Overflow` tracks how many entries are initialized and frees exactly
/// those in its `Drop`, so no exit path — success, `Decline`, or fatal error —
/// can leak a staged token or its state.
struct Overflow<T, N: ArrayLength> {
  slots: GenericArray<MaybeUninit<T>, N>,
  len: usize,
}

impl<T, N: ArrayLength> Overflow<T, N> {
  #[inline(always)]
  fn new() -> Self {
    Self {
      slots: GenericArray::uninit(),
      len: 0,
    }
  }

  // Only read by the debug-assertion accounting below; gate it to the same
  // configuration so release builds do not see it as dead code.
  #[cfg(debug_assertions)]
  #[inline(always)]
  fn len(&self) -> usize {
    self.len
  }

  /// Stages one owned entry. Callers must not exceed `N` pushes (the overflow
  /// region can never hold more than the window capacity).
  #[inline(always)]
  fn push(&mut self, value: T) {
    self.slots[self.len].write(value);
    self.len += 1;
  }

  /// Moves every staged entry into `buf`, in staging order, and disarms the
  /// guard so its `Drop` will not touch the moved-out entries.
  #[inline(always)]
  fn drain_into(self, buf: &mut GenericArrayDeque<T, N>) {
    // Wrap in `ManuallyDrop` up front: once entries are read out they must not be
    // dropped again by the guard.
    let this = ManuallyDrop::new(self);
    for i in 0..this.len {
      // SAFETY: `slots[0..len]` were initialized by `push`; each is read once.
      buf.push_back(unsafe { this.slots[i].assume_init_read() });
    }
  }
}

impl<T, N: ArrayLength> Drop for Overflow<T, N> {
  #[inline(always)]
  fn drop(&mut self) {
    for slot in self.slots.iter_mut().take(self.len) {
      // SAFETY: `slots[0..len]` were initialized by `push` and not moved out
      // (`drain_into` disarms via `ManuallyDrop`), so each is dropped once.
      unsafe { slot.assume_init_drop() };
    }
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Peeks the next token without advancing the cursor.
  #[inline]
  pub fn peek_one(
    &mut self,
  ) -> Result<
    Option<MaybeRefCachedTokenOf<'_, 'inp, L>>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  > {
    let mut buf = GenericArrayDeque::<_, U1>::new();
    self
      .peek_with_emitter_inner::<U1>(&mut buf, &mut false)
      .map(|_| buf.pop_front())
  }

  /// Peeks tokens to fill the provided buffer.
  ///
  /// If not enough tokens are cached, lexes more tokens to fill the buffer.
  /// The returned deque contains references to peeked tokens.
  ///
  /// # Partial mode: a short window, but never a hidden trip
  ///
  /// On a non-final [`Partial`](crate::input::Partial) input the fill stops at the frontier — a
  /// token touching the buffer end never enters the cache — so a peek there simply returns a
  /// **shorter window** than asked for; the [`Incomplete`](crate::error::Incomplete) surfaces when a
  /// consume path reaches the same frontier. A **terminal** condition is not held back that way: a
  /// limit trip during the fill emits its diagnostic and latches the poison boundary before the
  /// holdback is consulted, so a peek can no more hide a tripped limit than a consume can. See
  /// [terminal beats incomplete](crate::input#terminal-beats-incomplete-and-they-never-substitute).
  #[inline]
  pub fn peek<'p, W>(
    &'p mut self,
  ) -> Result<Peeked<'p, 'inp, L, W>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    W: Window,
  {
    self.peek_with_emitter::<W>().map(|(peeked, _)| peeked)
  }

  /// Peeks tokens to fill the provided buffer and returns the emitter.
  #[inline]
  pub fn peek_with_emitter<'p, W>(
    &'p mut self,
  ) -> Result<
    (Peeked<'p, 'inp, L, W>, &'p mut Ctx::Emitter),
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    W: Window,
  {
    let mut peeked = GenericArrayDeque::new();
    self
      .peek_with_emitter_inner::<W>(&mut peeked, &mut false)
      .map(|emitter| (peeked, emitter))
  }

  /// Peeks tokens to fill the window and reports whether the fill was cut short by a **terminal
  /// scanner stop** — a fresh resource-limit trip during the fill, or an already-latched poison
  /// boundary at the cursor.
  ///
  /// The peek contract is that a short window is not itself an error (it may be a genuine end of
  /// input, or a partial-input frontier). The returned flag lets a decision-window combinator tell
  /// the one case that class hides apart: a window truncated by a terminal stop is not evidence the
  /// construct ended, so such a combinator surfaces the committed end-of-input error rather than
  /// reading the short window as a decline. The flag is `true` only when the window came back
  /// *shorter than requested* because of a terminal stop; a full window, a genuine end of input, and
  /// a partial-input frontier holdback all report `false`.
  #[inline]
  pub fn peek_with_emitter_terminal<'p, W>(
    &'p mut self,
  ) -> Result<
    (Peeked<'p, 'inp, L, W>, bool, &'p mut Ctx::Emitter),
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    W: Window,
  {
    let mut peeked = GenericArrayDeque::new();
    let mut terminal = false;
    self
      .peek_with_emitter_inner::<W>(&mut peeked, &mut terminal)
      .map(|emitter| (peeked, terminal, emitter))
  }

  /// Internal implementation for peeking tokens.
  ///
  /// `terminal` is set to `true` iff the fill returned a window shorter than requested because of a
  /// terminal scanner stop (a fresh trip, or a pre-latched poison boundary) rather than a genuine
  /// end of input or a partial-input frontier — see
  /// [`peek_with_emitter_terminal`](Self::peek_with_emitter_terminal).
  #[inline]
  #[allow(unused_assignments)]
  fn peek_with_emitter_inner<'p, W>(
    &'p mut self,
    buf: &mut Peeked<'p, 'inp, L, W>,
    terminal: &mut bool,
  ) -> Result<&'p mut Ctx::Emitter, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    W: Window,
  {
    trace_event!(self, "peek");
    *terminal = false;
    let buf_len = buf.len();
    let remaining_cap = buf.capacity() - buf_len;
    let mut in_cache = self.cache().len();
    #[cfg(debug_assertions)]
    let initial_in_cache = in_cache;
    let mut want = remaining_cap.saturating_sub(in_cache);
    #[cfg(debug_assertions)]
    let exp = want;

    // If we already have enough tokens cached, just peek from cache
    if want == 0 {
      self.cache.peek::<W>(buf);
      return Ok(self.session.emitter);
    }

    // A sticky limit trip latches a poison boundary at the durable frontier: once
    // the cursor reaches it, never rebuild a lexer to scan past the trip. Serve
    // whatever is already cached and stop. The request went unmet at a latched
    // boundary — a terminal stop, not a genuine end of input.
    if self.reached_boundary(self.offset()) {
      *terminal = true;
      self.cache.peek::<W>(buf);
      return Ok(self.session.emitter);
    }

    // Drop-safe staging for tokens lexed past the cache window (see `Overflow`).
    let mut overflowed = Overflow::<MaybeRefCachedTokenOf<'p, 'inp, L>, W::CAPACITY>::new();
    // Set when a limit trip latches the input mid-scan: the staged overflow
    // tokens then become unreachable and must be truncated away (see below).
    let mut tripped = false;

    // Otherwise, lex additional tokens to fill the request. `lex_within_boundary`
    // stops the fill at the durable frontier during a replay, so an overflow peek
    // after a restore re-caches only the reproducible prefix.
    let mut lex_at = self.offset().clone();
    let mut lexer = self.lexer();
    while want > 0 {
      if let Some(item) = self.lex_within_boundary(&mut lexer, &mut lex_at) {
        // The one classifier ([`InputRef::classify`]), shared with the scanner: a terminal trip is
        // probed and LATCHED before the frontier holdback can withhold anything, so a peek can no
        // more disguise a limit trip as "more input may help" than a consume can. `AtCursor` is the
        // peek's frontier — a peek commits no progress, so a trip latches at the cursor, which
        // during a fill is the end of the last CACHED token (the staged overflow is not durable;
        // see the truncation below).
        match self.classify(&lexer, &AtCursor, item) {
          // Frontier holdback (partial, non-final), reached only by a NON-terminal item: it may
          // extend with more input, so it must never enter the cache — a later `next()` serves
          // cached tokens without re-lexing, which would bypass the scan-path holdback — nor be
          // emitted. Stop filling and withhold it; the peek returns a short window, and the
          // Incomplete surfaces when a consume path re-lexes the frontier via `scan_with`. This
          // preserves the invariant that the cache never holds a frontier token in this mode.
          // Const-gated: `Complete::PARTIAL` is `false`, so `classify` never builds this verdict on
          // the complete path and the arm is eliminated at monomorphization.
          Verdict::Withheld(_) => break,
          Verdict::Trip(err) => {
            // A limit trip is sticky, and `classify` has already latched the durable frontier — so
            // this (possibly fatal) emit cannot lose it: the `?` returns with the latch recorded for
            // every later operation, and `overflowed`'s `Drop` frees any staged tokens on the way
            // out.
            self.emit_lexer_error_deduped(err)?;
            tripped = true;
            break;
          }
          Verdict::Error(err) => {
            // Emit immediately regardless of cache fullness so an error in the
            // overflow region is never silently dropped. The dedup mark keeps a
            // later consume that re-lexes this region from reporting it twice.
            // `overflowed`'s `Drop` frees any staged tokens on this `?`-return.
            self.emit_lexer_error_deduped(err)?;
          }
          Verdict::Token(tok) => {
            let cached = CachedToken::new(tok, lexer.state().clone());

            // Try to cache the token; if cache is full, stage it for the output buffer
            match self.cache_push_back(cached) {
              Ok(()) => {
                in_cache += 1;
              }
              Err(ct) => {
                // Cache full: stage the overflow token drop-safely.
                overflowed.push(Maybe::Owned(ct));
              }
            }
            want -= 1;
          }
        }
      } else {
        break;
      }
    }

    // Fill buffer from cache (this covers both cached tokens and any we just added)
    // SAFETY: Cache.peek() returns slice of initialized tokens, guaranteed by trait contract
    self.cache.peek::<W>(buf);
    debug_assert!(
      buf_len + in_cache == buf.len(),
      "Cache peek returned unexpected number of tokens"
    );

    if tripped {
      // The fill was cut short by a fresh trip: report it terminal so a decision-window
      // combinator surfaces the stop instead of reading the short window as a decline.
      *terminal = true;
      // Truncate the result at the durability boundary. A limit trip latched the
      // input mid-overflow, so a post-peek `next()` will drain the cache-resident
      // prefix (already copied into `buf` above) and then stop — it can never
      // re-lex the staged overflow tokens. Handing them back would expose phantom
      // lookahead the caller can never consume, so drop them here instead. The
      // `Overflow` guard frees each staged token exactly once on this early
      // return; the `drain_into` hand-off below is skipped, so there is no
      // double-drop. This covers a trip on the first overflow token (nothing
      // staged) and a trip after several are staged alike.
      drop(overflowed);
      return Ok(self.session.emitter);
    }

    #[cfg(debug_assertions)]
    let yielded = overflowed.len();
    // Move the staged overflow tokens into the output buffer; `drain_into`
    // disarms the guard so nothing is double-dropped.
    overflowed.drain_into(buf);

    #[cfg(debug_assertions)]
    {
      debug_assert!(
        buf.len() == buf_len + in_cache + yielded,
        "buffer length mismatch after adding overflowed tokens"
      );
      if want == 0 {
        debug_assert!(
          exp == (in_cache - initial_in_cache) + yielded,
          "expected peeked token count mismatch"
        );
      }
    }

    Ok(self.session.emitter)
  }
}

#[cfg(all(test, feature = "logos", feature = "std"))]
mod tests;
