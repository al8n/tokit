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
  ///
  /// A token already waiting at the front of the stream — parked or cached — is served without
  /// touching the lexer.
  ///
  /// # It folds a terminal stop into `Ok(None)`
  ///
  /// This is the raw read: a resource-limit trip or a latched poison boundary is
  /// indistinguishable here from genuine end of input. A production that decides on the
  /// answer — "is there a `{` here?" — will read a halt as a grammar fact and keep going.
  /// Prefer [`peek_kind`](Self::peek_kind), [`head_satisfies`](Self::head_satisfies) or
  /// [`peek_head_map`](Self::peek_head_map), which raise on a terminal stop and reserve
  /// `Ok(None)` for the real end of input.
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
  ///
  /// Callers must consult the flag **immediately**, before any fallible or emitting call (`decide`,
  /// element handlers, close probes): an ordinary error raised in between preempts the terminal stop.
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
    // The parked front token is not a cache entry, but it IS the front of the stream, so it takes
    // a window slot and the fill must not re-lex it.
    let parked = usize::from(Self::can_park() && self.pending.is_some());
    let mut in_cache = self.cache().len();
    #[cfg(debug_assertions)]
    let initial_in_cache = in_cache;
    let mut want = remaining_cap.saturating_sub(parked + in_cache);
    #[cfg(debug_assertions)]
    let exp = want;

    // If enough tokens are already retained, just serve them
    if want == 0 {
      // The parked token is the front of the stream, so it heads the window and the cache fills in
      // behind it. Safe unguarded because every caller of this fill passes a buffer with room for at
      // least one entry.
      if Self::can_park() {
        if let Some(parked) = self.pending.as_ref() {
          buf.push_back(Maybe::Ref(parked.as_ref()));
        }
      }
      self.cache.peek::<W>(buf);
      return Ok(self.session.emitter);
    }

    // A sticky limit trip latches a poison boundary at the durable frontier: once
    // the cursor reaches it, never rebuild a lexer to scan past the trip. Serve
    // whatever is already cached and stop. The request went unmet at a latched
    // boundary — a terminal stop, not a genuine end of input.
    if self.reached_boundary(self.offset()) {
      *terminal = true;
      // The parked token is the front of the stream, so it heads the window and the cache fills in
      // behind it. Safe unguarded because every caller of this fill passes a buffer with room for at
      // least one entry.
      if Self::can_park() {
        if let Some(parked) = self.pending.as_ref() {
          buf.push_back(Maybe::Ref(parked.as_ref()));
        }
      }
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
    let mut resume = self.resume();
    // The lex position runs the fill loop as a BY-VALUE local (see `scan_with` for why: the opaque
    // per-token lexer call would otherwise force it to memory on every iteration); the slot is
    // written back after the loop. The two `?`-returns inside the loop skip that write-back, which
    // is sound because `resume` is function-local and dies with them.
    let ResumeParts { lexer, at: at_slot } = resume.parts_mut();
    let mut lex_at = at_slot.clone();
    while want > 0 {
      if let Some(item) = self.lex_within_boundary(lexer, &mut lex_at) {
        // The one classifier ([`InputRef::classify`]), shared with the scanner: a terminal trip is
        // probed and LATCHED before the frontier holdback can withhold anything, so a peek can no
        // more disguise a limit trip as "more input may help" than a consume can. `AtCursor` is the
        // peek's frontier — a peek commits no progress, so a trip latches at the cursor, which
        // during a fill is the end of the newest RETAINED token (the staged overflow is not
        // durable; see the truncation below).
        match self.classify(lexer, &AtCursor, item) {
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
            match self.cache_append(cached) {
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
    *at_slot = lex_at;

    // Fill the buffer from the front of the stream (this covers a parked token, the cached ones,
    // and any this fill just added)
    // SAFETY: Cache.peek() returns slice of initialized tokens, guaranteed by trait contract
    // The parked token is the front of the stream, so it heads the window and the cache fills in
    // behind it. Safe unguarded because every caller of this fill passes a buffer with room for at
    // least one entry.
    if Self::can_park() {
      if let Some(parked) = self.pending.as_ref() {
        buf.push_back(Maybe::Ref(parked.as_ref()));
      }
    }
    self.cache.peek::<W>(buf);
    debug_assert!(
      buf_len + parked + in_cache == buf.len(),
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
        buf.len() == buf_len + parked + in_cache + yielded,
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

  /// Width-1 head observation in grammar vocabulary, terminal-aware by construction.
  ///
  /// `f` sees the head as `Spanned<&Token, &Span>` and its value is returned. `Ok(None)`
  /// is genuine end of input; a **terminal stop** — a resource-limit trip or a latched
  /// poison boundary — raises the same terminal end-of-input error the `_or_stop` family
  /// raises, never a silent `None`. That distinction is the point: a consumer that reads
  /// a halt as "no head here" builds a value out of an input the scanner already gave up
  /// on.
  ///
  /// Rides the terminal-aware cache read
  /// ([`peek_with_emitter_terminal`](Self::peek_with_emitter_terminal)) rather than the
  /// `try_expect` scan, so the head is served from the front slot with no pop/hold
  /// round-trip.
  ///
  /// # A head already at the front of the stream is read where it lies
  ///
  /// This is a **width-1** read, and on a grammar's decision points the token it wants is almost
  /// always already there: measured on a GraphQL parse, 17,028 of 17,029 times. So the front of
  /// the stream is probed first, and a head that is there is handed straight to `f`.
  ///
  /// That is the same **head** the fill gives, by the fill's own construction — and therefore the
  /// same answer, for a caller who meets the condition below. With one token
  /// at the front — parked or cached — the window request is already met, so the fill takes its
  /// `want == 0` arm: it heads the window with the parked token if there is one and lets the
  /// cache fill in behind it, then returns. Nothing is lexed, nothing is committed, the terminal
  /// flag stays `false` (that arm returns *before* the boundary probe, so a resident head is
  /// served whatever the poison latch says), and [`Cache::peek`](crate::cache::Cache::peek) is a
  /// `&self` read the trait documents as logically pure. All the shared route adds under that
  /// condition is arithmetic over the window slots, an overflow guard that stages nothing, and a
  /// copy of the entry into a `GenericArrayDeque` that is popped straight back out — and the
  /// token that copy denotes is the one handed over here.
  ///
  /// The end-of-input offset the fill's `None` arms need is not read on this route because those
  /// arms are unreachable with a head in hand; the trace hook the fill opens with is emitted here
  /// instead, so a `trace` build sees the same one event per call either way.
  ///
  /// ## What is guaranteed identical, and the condition that makes it so
  ///
  /// The same condition [`skip_while`](Self::skip_while) states applies here, and the difference
  /// it covers is much the smaller of the two: a peek commits nothing, so there is no frontier to
  /// clone and no mark to take on **either** route. What differs is confined to the cache surface
  /// and one offset read. Measured, on the same stream in the same residency, by the effect ledger
  /// in `fast_path_tests`:
  ///
  /// | caller-supplied step, for one width-1 read | this route | the general route |
  /// |---|---|---|
  /// | `Cache` | one `front` | one `len`, one `peek` — the fill's `want == 0` arm |
  /// | `L::Offset::clone` | 0 | 1 — the committed span's end, hoisted *here* above the fill |
  /// | `L::Span::clone`, `L::State::clone` | 0 | 0 |
  /// | `Emitter::checkpoint` / `release` / any emission | none | none |
  ///
  /// As on [`skip_while`](Self::skip_while), **that table is a measurement and not a boundary**:
  /// it holds the steps the ledger was built to watch, and the clause below is about every
  /// caller-supplied operation whether a table names it or not.
  ///
  /// ### What this route does differently — the whole of it
  ///
  /// Three clauses, meant as exhaustive. Against the general route, for every call it answers,
  /// this route:
  ///
  /// 1. **omits** caller-supplied steps and adds none. The hoisted `L::Offset::clone` is the one
  ///    the ledger sees; as with clause (1) on `skip_while`, *which* steps is not part of the
  ///    clause — it is a subset relation, not a list;
  /// 2. **substitutes** one [`Cache::front`](crate::cache::Cache::front) for the fill's
  ///    [`len`](crate::cache::Cache::len) + [`peek`](crate::cache::Cache::peek). All three are
  ///    `&self` reads the cache contract defines as changing no observable, so they are the same
  ///    read of the same head;
  /// 3. hands `f` the **same token and the same span**, once.
  ///
  /// ### The condition on the caller
  ///
  /// What follows holds for a caller who meets both clauses of the condition on
  /// [`skip_while`](Self::skip_while), read here with `f` in the predicate's place:
  ///
  /// * **your input-layer callbacks are inert** — every caller-supplied operation this crate can
  ///   reach through the input layer does only what its own contract says, and always returns
  ///   normally: no unwind, no divergence. *All of it, not a list.* Likely to be yours: the
  ///   `Clone`, `Drop`, `Ord` and `Hash` of `L::Offset` and `L::Span`, the `Clone` and `Drop` of
  ///   `L::State`, every [`Cache`](crate::cache::Cache) method, the [`Emitter`](crate::Emitter),
  ///   the [`Lexer`](crate::Lexer) and its [`Source`](crate::Source) — named as the ones you are
  ///   likely to write, not as the edge of the clause;
  /// * **`f` is a function of what it is handed** — it answers from the *values* of the
  ///   `Spanned<&L::Token, &L::Span>` it receives, not from state another callback wrote and not
  ///   from the addresses those references carry.
  ///
  /// The closure argument is the same one, and it is why this is a condition rather than a list:
  /// the three clauses above say the crate hands `f` the same values, so any difference must come
  /// from caller-supplied code; the first clause makes all of that code invisible whether it runs
  /// or not, and the second stops `f` reading which route produced its argument. `F: FnOnce(..)
  /// -> O` may capture whatever it likes and `L::Offset` is *your* type, so neither clause is a
  /// formality — but both are properties of your own types, checkable once.
  ///
  /// ### For such a caller, guaranteed identical
  ///
  /// The value handed to `f`, and therefore the value returned; that `f` runs exactly once; that
  /// nothing is consumed, committed, emitted or latched; that a resident head is served at a
  /// latched poison boundary and a non-resident one still raises the terminal end-of-input error.
  ///
  /// ### And for a caller who does not meet it
  ///
  /// Both of these are one clause failing, and both are measured. Neither is the list of ways to
  /// fail — a caller who breaks a clause some other way gets the same answer:
  ///
  /// * **an `f` that reads the input layer** — the second clause. The offset row above is the
  ///   whole mechanism: the general route takes `self.span().end()` before the fill, for a
  ///   terminal end-of-input error a resident head makes unreachable, and this route does not, so
  ///   the returned `O` differs between them. An `O` that differs is a parse that differs —
  ///   [`head_satisfies`](Self::head_satisfies) and [`peek_kind`](Self::peek_kind) ride this call,
  ///   so the value in question is routinely a grammar decision.
  ///   (`an_offset_clone_counting_f_can_change_the_value_peek_head_map_returns`)
  /// * **an `L::Offset::clone` that unwinds** — the first clause, and again the sharper case,
  ///   because such a caller's `f` may observe nothing whatsoever. That hoisted clone is the
  ///   general route's *first* caller step: arm it to panic and the general route unwinds before
  ///   `f`, where this route never reads the offset and returns `Ok(Some(_))` with `f` run once.
  ///   Whether `f` ran at all is then decided by which route answered.
  ///   (`an_unwinding_offset_clone_decides_whether_peek_head_maps_closure_runs_at_all`)
  ///
  /// An `f` whose answer — or whose *reachability* — depends on **how the input layer got the head
  /// to it** is not asking about the head; it is asking which route this crate took, and that is a
  /// choice this crate makes and may change in any release. Answer out of the
  /// `Spanned<&L::Token, &L::Span>` you were handed, from types that clone by copying, and both
  /// clauses hold by construction — as they do for every `f` in this crate, its tests and its
  /// examples.
  #[inline]
  pub fn peek_head_map<O, F>(
    &mut self,
    f: F,
  ) -> Result<Option<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    F: FnOnce(Spanned<&L::Token, &L::Span>) -> O,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    use crate::cache::PeekedTokenExt as _;
    // ── The resident head, read in place (see the section above) ──
    if let Some(front) = self.front() {
      trace_event!(self, "peek");
      return Ok(Some(f(front.token)));
    }
    // Hoisted before the fill: the committed span does not move under a pure peek, and
    // the window borrow (`peeked` ties to `&mut self`) must not overlap the read.
    let end = self.span().end();
    let (mut peeked, terminal, _emitter) =
      self.peek_with_emitter_terminal::<generic_arraydeque::typenum::U1>()?;
    match peeked.pop_front() {
      Some(head) => {
        let out = f(Spanned::new(head.span(), head.token()));
        Ok(Some(out))
      }
      None if terminal => Err(UnexpectedEot::eot_of(end).into_terminal().into()),
      None => Ok(None),
    }
  }

  /// Does the head satisfy `pred`?
  ///
  /// `false` at genuine end of input; a terminal stop is an error. Replaces the
  /// consumer-side always-decline `try_expect` hack, which answered `false` for both.
  #[inline]
  pub fn head_satisfies<F>(
    &mut self,
    pred: F,
  ) -> Result<bool, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    F: FnOnce(&L::Token) -> bool,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    self
      .peek_head_map(|sp| pred(sp.data))
      .map(|o| o.unwrap_or(false))
  }

  /// The head's kind, on the same terminal-aware read as
  /// [`peek_head_map`](Self::peek_head_map).
  ///
  /// The method form of the free [`peek_kind`](crate::parser::peek_kind). Note the
  /// contract difference from a hand-rolled `peek::<U1>()` fork: that discards the
  /// terminal flag, so a latched boundary reads as `Ok(None)`; this raises.
  #[inline]
  pub fn peek_kind(
    &mut self,
  ) -> Result<
    Option<<L::Token as Token<'inp>>::Kind>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    self.peek_head_map(|sp| sp.data.kind())
  }
}

#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod tests;
