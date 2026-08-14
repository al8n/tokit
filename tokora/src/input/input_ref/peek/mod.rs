use generic_arraydeque::GenericArrayDeque;

use super::*;

/// CACHE_COPY's release half, raised **out of line**.
///
/// `#[cold]` and `#[inline(never)]` are load-bearing rather than decorative, and for a
/// reason that is this module's whole subject: a formatted `panic!` needs a
/// `core::fmt::Arguments` and its argument array built somewhere, and inlined into the fill
/// that somewhere is the *fill's* frame — 96 bytes on aarch64, reserved on every call for a
/// path that never runs. PEEK_FOOTPRINT is a frame-size law, so the panic's own storage
/// belongs in the panic's own function. Measured: with this message inlined, a `U1` peek's
/// release frame *grew* by those 96 bytes and swallowed the window this change was made to
/// save.
///
/// Free-standing rather than an associated function for the same accounting: it names none
/// of `InputRef`'s eight parameters, so the crate carries one copy of it instead of one per
/// monomorphization.
#[cold]
#[inline(never)]
fn cache_copy_count_violation(reserved: usize, copied: usize) -> ! {
  panic!(
    "`Cache` contract violation: the peek fill counted {reserved} resident entr(ies) from \
     `Cache::len` and left `Cache::peek` room for all of them — and `Cache::peek` appended \
     {copied}. `len` must be exactly the resident count and `peek` must append exactly \
     `min(len(), the buffer's remaining capacity)`"
  )
}

/// CACHE_COPY's endpoint half, raised **out of line**, for exactly the reason
/// [`cache_copy_count_violation`] is.
///
/// This message is the longer of the two and takes two runtime arguments, so inlined it
/// reserves its `core::fmt::Arguments` and argument array in the *fill's* frame — the 96-byte
/// aarch64 cost measured on the count message, on a path that never runs. Free-standing and
/// non-generic for the same accounting: one copy in the crate rather than one per
/// monomorphization. It is also what keeps the release-active witness at its call site down to
/// the compares themselves plus a never-taken branch to here.
#[cold]
#[inline(never)]
fn cache_copy_endpoint_violation(copied: usize, staged: usize) -> ! {
  panic!(
    "`Cache` contract violation: `Cache::peek` did not copy the cache's whole resident run. \
     The {copied} entr(ies) it appended do not span the cache from `front` to `back`, and \
     {staged} staged token(s) are about to be rotated in behind them — so the window would \
     report a token at a position the next consume will not serve. That is what an inexact \
     `Cache::len` does to this copy: the fill reserves the room from `len` before it stages \
     anything, so a `len` below the resident count clips the copy mid-run"
  )
}

/// Stages one overflow token at the tail of the window, out of line.
///
/// `#[inline(never)]` for the same reason [`cache_copy_count_violation`] is: this arm runs
/// only where the cache has refused a token, and a ring push's head/len arithmetic and
/// capacity branch inlined into the fill loop shape the register allocation of the arm that
/// runs on every ordinary fill.
#[inline(never)]
fn stage_overflow<T, N: generic_arraydeque::ArrayLength>(
  buf: &mut GenericArrayDeque<T, N>,
  value: T,
) {
  // The fill's accounting proves the push is always accepted (see PEEK_FOOTPRINT); assert the
  // room in debug builds so a future change to it cannot silently drop a token. The predicate
  // is the window's own two numbers, not the cache's.
  debug_assert!(
    buf.len() < buf.capacity(),
    "peek staged an overflow token past the window capacity"
  );
  buf.push_back(value);
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
  /// token the lexer decided by reading as far as the buffer end
  /// ([`read_frontier`](crate::Lexer::read_frontier), floored at the item's own span end) never
  /// enters the cache — so a peek there simply returns a **shorter window** than asked for. How
  /// much shorter is the lexer's to say, not the span's: a lookahead lexer holds back items whose
  /// spans sit well behind the buffer end, and one reporting
  /// [`Unbounded`](crate::ReadFrontier::Unbounded) caches nothing at all until the stream is
  /// sealed. The [`Incomplete`](crate::error::Incomplete) surfaces when a
  /// consume path reaches the same frontier. A **terminal** condition is not held back that way: a
  /// limit trip during the fill emits its diagnostic and latches the poison boundary before the
  /// holdback is consulted, so a peek can no more hide a tripped limit than a consume can. See
  /// [terminal beats incomplete](crate::input#terminal-beats-incomplete-and-they-never-substitute).
  ///
  /// ## What that leaves: the return value cannot tell the three apart
  ///
  /// The paragraph above is about the **diagnostic**, and it is the whole truth about the
  /// diagnostic. It is not a statement about this **return type**. A full window, a genuinely short
  /// one, a partial-input frontier holdback and a terminal stop all arrive here as `Ok` holding a
  /// window, and the last two are the same length for the same reason nothing distinguishes them:
  /// the short window *is* the value. A production that decides on the width — *"is the second
  /// token a `(`?"* — therefore reads a halted scanner as a grammar fact and picks the other
  /// production, which is [`peek_one`](Self::peek_one)'s `Ok(None)` fold one width up.
  ///
  /// With a **fatal** emitter that is invisible, because the trip's own diagnostic ends the parse
  /// either way. With a **non-fatal** emitter that accepts it, the caller is handed `Ok` with fewer
  /// tokens than it asked for and no way to ask why — a silently different parse rather than a
  /// stopped one.
  ///
  /// Two things can express it. [`peek_with_emitter_terminal`](Self::peek_with_emitter_terminal)
  /// reports it as a flag beside the window, for a caller that wants the short window *and* the
  /// reason; [`peek_map`](Self::peek_map) puts it in the error arm, reserving a short `Ok` window
  /// for a genuine end of input, which is what [`peek_head_map`](Self::peek_head_map),
  /// [`peek_kind`](Self::peek_kind) and [`head_satisfies`](Self::head_satisfies) do at the head.
  /// Prefer one of them wherever the window's *length* decides a production.
  ///
  /// # Stack footprint: one window, cache hit or miss
  ///
  /// The window this returns is the **only** `W::CAPACITY`-sized owned token storage a peek
  /// reserves, and its worst case is the whole array live at once:
  ///
  /// ```text
  /// W::CAPACITY × size_of::<Maybe<CachedToken<&Token, &State, &Span>,
  ///                              CachedToken<Token, State, Span>>>()
  /// ```
  ///
  /// which for every realistic type is `W::CAPACITY × (size_of::<Token>() + size_of::<State>() +
  /// size_of::<Span>())` plus per-entry padding and a discriminant. A **cache miss costs no
  /// second window**: tokens lexed past the cache are staged in this same buffer and rotated
  /// into place, so the miss path reserves exactly what the hit path does. (Through 0.7.3 the
  /// miss path staged them in a separate `W::CAPACITY`-slot array, doubling the figure above.)
  ///
  /// Everything else in the frame is **O(1) in the window width**: single-entry temporaries (one
  /// [`CachedToken`](crate::cache::CachedToken) in flight to the cache, the deque's own push and
  /// rotate temporaries), one clone of the lexer — `size_of::<L>()`, which contains `State` — and
  /// a small fixed part. Nothing here is heap-allocated, and nothing scales with the input.
  ///
  /// `Token` and `State` are unconstrained in size, so the bound is linear in both and in the
  /// window width — with a coefficient of **one**, not two. A grammar carrying a large token
  /// payload or a large lexer state pays `W::CAPACITY` times it for the width it asks for:
  /// prefer the narrowest window that decides the production, and
  /// [`peek_kind`](Self::peek_kind) or [`head_satisfies`](Self::head_satisfies) — which run at
  /// `U1` — for a head test.
  ///
  /// # Panics
  ///
  /// On a [`Cache`](crate::cache::Cache) that breaks its own contract, and only then, on the
  /// fill path that reaches the lexer. The fill reserves the window's cache region from
  /// [`Cache::len`](crate::cache::Cache::len) *before* it stages anything past it, so a `len`
  /// that is not the resident count mis-sizes the room
  /// [`Cache::peek`](crate::cache::Cache::peek) is then given. The exit that hands such a window
  /// back checks the copy that landed in it — against the room the fill left, and against the
  /// cache's own `front` and `back`, which an inexact `len` cannot move — and panics rather than
  /// return a window that is wrong about the stream. **Both checks run in release**, because the
  /// window a broken `len` produces there is not a short one but a hole. Every cache this crate
  /// ships conforms, and the cache conformance kit checks a downstream one.
  #[inline]
  pub fn peek<'p, W>(
    &'p mut self,
  ) -> Result<Peeked<'p, 'inp, L, W>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    W: Window,
  {
    self.peek_with_emitter::<W>().map(|(peeked, _)| peeked)
  }

  /// Peeks tokens to fill the provided buffer and returns the emitter's **operations**.
  ///
  /// The second half is an [`EmitterView`], not the emitter: the value a `*_while` condition is
  /// handed. Returning `&mut Ctx::Emitter` here would be the same door
  /// `InputRef::emitter` is crate-private to shut — see [`EmitterView`] for the class.
  ///
  /// Reserves the same one owned window as [`peek`](Self::peek), cache hit or miss, and panics
  /// on the same broken-`Cache` condition — see its stack-footprint and panics sections.
  #[inline]
  pub fn peek_with_emitter<'p, W>(
    &'p mut self,
  ) -> Result<
    (
      Peeked<'p, 'inp, L, W>,
      EmitterView<'p, 'inp, L, Ctx::Emitter, Lang>,
    ),
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    W: Window,
  {
    let mut peeked = GenericArrayDeque::new();
    self
      .peek_with_emitter_inner::<W>(&mut peeked, &mut false)
      .map(|emitter| (peeked, EmitterView::new(emitter)))
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
  ///
  /// Rides the same fill as [`peek`](Self::peek) — the terminal flag is an extra out-parameter on
  /// it, not a second code path — so it reserves the same one owned window, cache hit or miss, and
  /// panics on the same broken-`Cache` condition. See its stack-footprint and panics sections.
  #[inline]
  pub fn peek_with_emitter_terminal<'p, W>(
    &'p mut self,
  ) -> Result<
    (
      Peeked<'p, 'inp, L, W>,
      bool,
      EmitterView<'p, 'inp, L, Ctx::Emitter, Lang>,
    ),
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    W: Window,
  {
    let mut peeked = GenericArrayDeque::new();
    let mut terminal = false;
    self
      .peek_with_emitter_inner::<W>(&mut peeked, &mut terminal)
      .map(|emitter| (peeked, terminal, EmitterView::new(emitter)))
  }

  /// Internal implementation for peeking tokens.
  ///
  /// This one still hands back `&mut Ctx::Emitter`: it is private, and its two public callers wrap
  /// the reference in an [`EmitterView`] on the way out. The wall is at the crate boundary, not
  /// inside the fill.
  ///
  /// `terminal` is set to `true` iff the fill returned a window shorter than requested because of a
  /// terminal scanner stop (a fresh trip, or a pre-latched poison boundary) rather than a genuine
  /// end of input or a partial-input frontier — see
  /// [`peek_with_emitter_terminal`](Self::peek_with_emitter_terminal).
  #[inline]
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
    let in_cache = self.cache().len();
    let want = remaining_cap.saturating_sub(parked + in_cache);

    // If enough tokens are already retained, just serve them
    if want == 0 {
      // The parked token is the front of the stream, so it heads the window and the cache fills in
      // behind it. Safe unguarded because every caller of this fill passes a buffer with room for at
      // least one entry.
      if Self::can_park()
        && let Some(parked) = self.pending.as_ref()
      {
        buf.push_back(Maybe::Ref(parked.as_ref()));
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
      if Self::can_park()
        && let Some(parked) = self.pending.as_ref()
      {
        buf.push_back(Maybe::Ref(parked.as_ref()));
      }
      self.cache.peek::<W>(buf);
      return Ok(self.session.emitter);
    }

    // The flag comes back as a value rather than through the `&mut bool`: `peek_one` passes a
    // `&mut false` it never reads, and with the fill out of line an out-parameter would force
    // that temporary into memory and a pointer into the call. Returned, both writes to it are
    // dead stores the inliner drops.
    self
      .peek_lex_fill::<W>(buf, buf_len, parked, in_cache, want)
      .map(|(emitter, term)| {
        *terminal = term;
        emitter
      })
  }

  /// PEEK_HOT_SPLIT — the half of the fill that reaches the lexer, kept **out of line**.
  ///
  /// The two arms above answer from tokens already at the front of the stream, and on a
  /// grammar's decision points that is nearly always the answer: measured on a GraphQL parse,
  /// 17,028 width-1 reads out of 17,029. This half is the other one.
  ///
  /// `#[inline(never)]` is a measurement, not a preference. Staging the overflow region in
  /// `buf` puts the deque's push, truncate and rotate — and a `Cache::peek` whose room is now a
  /// runtime value rather than a constant — into the same body as those two arms, and the
  /// register allocation and block layout that follow are shared. Left in one function the
  /// resident-head arm picked up about seven instructions on a thirty-instruction path it does
  /// not use, and six peek-shaped bench ids paid **1.11×–1.31×** for a change that never
  /// touched their code. Split out — with the overflow-free exit below taking its own tail and
  /// the panic message built in [`cache_copy_count_violation`] — the same six read 0.80×–1.15×,
  /// geometric mean 0.98×, and the two `heavy` ids come back 20% faster than the two-window
  /// version they replace. The extra call is paid only where a lexer call is about to dwarf it.
  /// (The cost is *not* the CACHE_COPY checks: with them removed entirely the same six ids
  /// still read 1.11×–1.31×.)
  ///
  /// The receiver is `&'p mut self` — the *caller's own* borrow, not a fresh reborrow — because
  /// the entries [`Cache::peek`](crate::cache::Cache::peek) appends borrow the cache for `'p`,
  /// the window's element lifetime. The return type names `'p`, which is what forces the
  /// reborrow to be that long.
  ///
  /// # Parameters
  ///
  /// The four quantities the caller already computed, so this body does not recompute them:
  /// `buf_len` (the caller's own prefix, excluded from everything here), `parked`, `in_cache`
  /// (the cache's reported resident count) and `want` (window slots left to fill, `> 0`).
  #[inline(never)]
  fn peek_lex_fill<'p, W>(
    &'p mut self,
    buf: &mut Peeked<'p, 'inp, L, W>,
    buf_len: usize,
    parked: usize,
    in_cache: usize,
    want: usize,
  ) -> Result<(&'p mut Ctx::Emitter, bool), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    W: Window,
  {
    let mut terminal = false;
    let mut in_cache = in_cache;
    let mut want = want;
    #[cfg(debug_assertions)]
    let initial_in_cache = in_cache;
    #[cfg(debug_assertions)]
    let exp = want;

    // PEEK_FOOTPRINT — WHY THE OVERFLOW REGION LIVES IN `buf`.
    //
    // The two halves of a window are produced in the opposite order from the one it must
    // report: the cache region is copied out in one bulk read *after* the fill, while the
    // tokens past the cache are lexed *during* it. That ordering — not a shortage of room —
    // is what a staging area exists to solve. It is solved here by staging those tokens at
    // `buf`'s own tail and, once the cache region has been copied in behind them, rotating
    // them back to the end (see after the loop).
    //
    // Staging in `buf` rather than in a second `W::CAPACITY`-slot array is what holds the peek
    // frame to ONE window-sized store: `Token` and `State` are unconstrained in size, so a
    // second one doubles a peek's stack for a grammar that carries a large payload. It is also
    // why this function needs no partial-initialization bookkeeping: `buf` owns each staged
    // entry from the moment it is pushed, so every exit path — success, a withheld frontier, a
    // trip, or a failing return from a fatal emit — frees it exactly once through the deque's
    // own `Drop`.
    //
    // The copy is NOT reorderable ahead of the loop. `Cache::peek` takes `&'p self` and the
    // entries it appends borrow the cache for `'p`, the whole of the fill; the loop's every
    // step (`lex_within_boundary`, `classify`, `emit_lexer_error_deduped`, `cache_append`)
    // takes `&mut self`, and `cache_append` genuinely mutates the cache it would be aliasing.
    //
    // The staged region always fits. With `cap = buf.capacity()`, the fill is entered only with
    // `want = cap - buf_len - parked - in_cache` (the `saturating_sub` above returns early at
    // zero), and every token the loop produces decrements `want` and lands in either the cache
    // (`in_cache += 1`) or the staged region. So at every point
    // `buf_len + staged + parked + in_cache <= cap`, and the per-push assertion below pins the
    // same bound locally. Note what that accounting reserves: the cache region's room as much
    // as the staged region's, sized from `Cache::len()` before a single token is staged. A
    // `Cache` whose `len` is not its resident count therefore mis-sizes the copy that comes
    // after the loop — see CACHE_COPY there for the checks that refuse to rotate on one.
    let mut staged = 0usize;
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
            // this (possibly fatal) emit cannot lose it: the failing return below carries the latch
            // into every later operation.
            if let Err(err) = self.emit_lexer_error_deduped(err) {
              // Unstage before propagating. Nothing but the staged tokens has been pushed yet, so
              // `buf` holds exactly `buf_len + staged` entries and truncating to `buf_len` drops
              // each staged token exactly once (the deque owns them) and hands the caller's buffer
              // back byte-for-byte as it arrived — the same observable the discarded staging
              // buffer used to produce.
              buf.truncate(buf_len);
              return Err(err);
            }
            tripped = true;
            break;
          }
          Verdict::Error(err) => {
            // Emit immediately regardless of cache fullness so an error in the
            // overflow region is never silently dropped. The dedup mark keeps a
            // later consume that re-lexes this region from reporting it twice.
            if let Err(err) = self.emit_lexer_error_deduped(err) {
              // Unstage before propagating; see the `Trip` arm for why `buf_len` is the exact
              // pre-fill length here.
              buf.truncate(buf_len);
              return Err(err);
            }
          }
          Verdict::Token(tok) => {
            let cached = CachedToken::new(tok, lexer.state().clone());

            // Try to cache the token; if cache is full, stage it for the output buffer
            match self.cache_append(cached) {
              Ok(()) => {
                in_cache += 1;
              }
              Err(ct) => {
                // Cache full: stage the overflow token at the tail of the output buffer.
                stage_overflow::<_, W::CAPACITY>(buf, Maybe::Owned(ct));
                staged += 1;
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

    if tripped {
      // The fill was cut short by a fresh trip: report it terminal so a decision-window
      // combinator surfaces the stop instead of reading the short window as a decline.
      terminal = true;
      // Truncate the result at the durability boundary. A limit trip latched the
      // input mid-overflow, so a post-peek `next()` will drain the cache-resident
      // prefix (copied in just below) and then stop — it can never re-lex the
      // staged overflow tokens. Handing them back would expose phantom lookahead
      // the caller can never consume, so drop them here instead. Nothing but the
      // staged tokens has been pushed yet, so `buf_len` is the exact pre-fill
      // length and the truncation frees each staged token exactly once. This
      // covers a trip on the first overflow token (nothing staged) and a trip
      // after several are staged alike.
      buf.truncate(buf_len);
      staged = 0;
    }

    // Fill the buffer from the front of the stream (this covers a parked token, the cached ones,
    // and any this fill just added)
    // The parked token is the front of the stream, so it heads the window and the cache fills in
    // behind it. Safe unguarded because every caller of this fill passes a buffer with room for at
    // least one entry.
    if Self::can_park()
      && let Some(parked) = self.pending.as_ref()
    {
      buf.push_back(Maybe::Ref(parked.as_ref()));
    }
    debug_assert!(
      buf.len() == buf_len + staged + parked,
      "the fill pushed something it did not account for before the cache copy"
    );

    if staged == 0 {
      // PEEK_HOT_SPLIT — NOTHING OVERFLOWED, which is what a fill that fits the cache always
      // looks like, and what a trip leaves behind after the truncation above. `buf` holds
      // exactly what it held here before the reordering — the caller's prefix and the parked
      // token — so this exit *is* the one 0.7.3 shipped: nothing to rotate, and no CACHE_COPY
      // hazard, because a copy with nothing behind it can shorten the window but cannot hole
      // it (the same reason the two arms above are unchecked).
      //
      // Its own arm rather than a fold into the general one, and the difference is not
      // cosmetic: with `staged` a runtime value the room this copy is given stops being a
      // constant, which is enough to cost the copy its specialization. Six peek-shaped bench
      // ids — the dispatch family, which drains as it goes and so reaches this fill on every
      // token — paid 1.11×–1.31× for exactly that, with the checks removed entirely.
      self.cache.peek::<W>(buf);
      #[cfg(debug_assertions)]
      {
        debug_assert!(
          buf.len() == buf_len + parked + in_cache,
          "buffer length mismatch after the cache copy"
        );
        if want == 0 {
          debug_assert!(
            exp == in_cache - initial_in_cache,
            "expected peeked token count mismatch"
          );
        }
      }
      return Ok((self.session.emitter, terminal));
    }

    let cache_copy_at = buf.len();
    self.cache.peek::<W>(buf);
    Self::assert_cache_copy::<W>(self.cache, buf, cache_copy_at, in_cache, staged);

    // Restore stream order. The staged tokens were appended *before* the front-of-stream
    // region, so the region this fill owns currently reads `[staged…][parked?][cache…]` where
    // the window must read `[parked?][cache…][staged…]`. One left-rotation by `staged` over
    // that region (`buf_len..`) is exactly that permutation, and it permutes values inside the
    // deque — nothing is copied out, so no entry can be leaked or dropped twice. Entries the
    // caller passed in (`..buf_len`) are excluded, so a prefilled buffer keeps its order too.
    buf.make_contiguous()[buf_len..].rotate_left(staged);

    #[cfg(debug_assertions)]
    {
      debug_assert!(
        buf.len() == buf_len + parked + in_cache + staged,
        "buffer length mismatch after the cache copy and the staged region"
      );
      if want == 0 {
        debug_assert!(
          exp == (in_cache - initial_in_cache) + staged,
          "expected peeked token count mismatch"
        );
      }
    }

    Ok((self.session.emitter, terminal))
  }

  /// CACHE_COPY — the window invariant on the one exit that reorders, checked there and
  /// nowhere else.
  ///
  /// # The invariant
  ///
  /// > A peek window is a **contiguous prefix of the token stream at the cursor**. On the fill
  /// > exit the cache region is not the window's tail — the staged region is rotated in behind
  /// > it — so that region must be the cache's **whole resident run**, `front` through `back`.
  /// > A capacity clip is not licensed here and cannot happen on a conforming cache: by the
  /// > fill's own arithmetic the room left for the copy is `in_cache + want_unspent`, which is
  /// > never below `in_cache`.
  ///
  /// # Why a caller-facing check exists at all, and only here
  ///
  /// [`Cache::len`](crate::cache::Cache::len) is load-bearing on this path: the fill reserves the
  /// window's cache region from it *before* it stages anything, spends the rest of the window on
  /// tokens lexed past the cache, and only then calls [`Cache::peek`](crate::cache::Cache::peek)
  /// into the room that is left. A `len` that is not the resident count mis-sizes that copy, and
  /// the two directions fail differently:
  ///
  /// - an **under-report** clips the copy mid-run, and the rotation then closes the gap with
  ///   staged tokens that belong *after* the residents the clip dropped — not a short window, a
  ///   **HOLE**: the window reports a token at a position where the next consume serves a
  ///   different one. This failure is **created by the reordering** and is the reason this
  ///   function exists;
  /// - an **over-report** makes the fill reserve slots for residents that are not there and lex
  ///   too few tokens for them, so the window comes back **SHORT** while the input still has
  ///   more. That one is not new — it is what an over-reporting cache did before the reordering
  ///   too — but the count below sees it for free.
  ///
  /// The two early exits of the fill (the cache hit and the latched boundary) append nothing
  /// behind the copy, so neither can hole and neither is checked: they behave exactly as they
  /// did before the reordering, and adding a check there would put its cost on the width-1 head
  /// read a grammar runs per token.
  ///
  /// # Why the guards are shaped the way they are
  ///
  /// Every quantity a check is *gated* on is one the **fill** computes — `at`, `copied`,
  /// `staged` — never one the cache reports. `reserved` is a cache-derived value and appears
  /// only on the *expected* side of a comparison, where a lie shows up as a mismatch; it never
  /// decides whether a comparison runs. That is not a style preference, it is the correction of
  /// a real defect: an earlier revision of this check gated the endpoint witness on
  /// `reserved > 0`, and a cache under-reporting all the way to **zero** skipped the witness by
  /// that precondition while passing the count trivially as `0 == 0` — blindest exactly where
  /// the lie was largest. A precondition is an escape hatch unless it is itself verified.
  ///
  /// # The two halves, both in every build
  ///
  /// **THE COUNT** is a compare in every build. It catches the over-report direction outright,
  /// and it catches every under-report except the one that lands exactly on the window's edge.
  ///
  /// **THE ENDPOINTS** — the resident run's own witness, read from `front`/`back` rather than
  /// from `len`, so an inexact `len` cannot both cause the fault and hide it — is in every build
  /// too. It has to be, because it is the ONLY check that can see the two cases the count cannot:
  /// a clip whose `copied` happens to equal the reserved count, and an under-report all the way
  /// to zero, which satisfies the count trivially as `0 == 0`. Both of those are the **hole**, and
  /// a release build is where a downstream `Cache` runs.
  ///
  /// It was a `debug_assert!` for one release, on an instruction count that read the comparison —
  /// two ring indexes, a `Maybe` discriminant, an `Option<&Span>` compare — at 67–70 instructions
  /// retired per cache-miss fill. **That reasoning is retracted, and the retraction is measured.**
  /// The instruction count was never the price: the six peek-shaped bench ids read **0.9987**
  /// geometric mean with this witness release-active, every id inside 0.974×–1.009×, over ten
  /// interleaved criterion rounds on aarch64-apple-darwin.
  ///
  /// What the promotion *did* cost, before `#[inline(never)]` below, was the same thing
  /// PEEK_HOT_SPLIT found: the witness's code sitting in
  /// [`peek_lex_fill`](Self::peek_lex_fill)'s body — past the
  /// overflow-free `return`, never executed by it — moved `input/scan/peek1_then_next` by
  /// **1.025×**, consistently and outside the noise. Out of line it reads **1.003×**. An
  /// instruction count answers "what does this code cost where it runs"; the question that
  /// decides a hot path is "what does its presence cost the arm that skips it", and only a
  /// measurement answers that one.
  ///
  /// # Parameters
  ///
  /// - `at` — where the copy began: `buf.len()` immediately before `Cache::peek`.
  /// - `reserved` — the resident count the fill believes the cache has: `Cache::len()` read
  ///   before the fill, plus every token the fill then appended. Untrusted.
  /// - `staged` — how many tokens are waiting to be rotated in behind the copy, and the whole
  ///   reason this function is called: the fill reaches it only where `staged > 0`, because
  ///   that is exactly where a clipped copy becomes a hole rather than a short tail. Reported
  ///   in the message; not read as a gate, since the gate is the call site.
  ///
  /// # Panics
  ///
  /// On a `Cache` that breaks the contract above, in **every build**, on either half — the
  /// posture [`InputRef::park`](super::InputRef) takes for a `RETAINS_FRONT` refusal and
  /// [`Sink::cst_start`](crate::cst::Sink::cst_start) takes for a node kind. The violation is in
  /// caller-supplied trait code, no caller can repair its own `Cache` mid-parse, and the
  /// alternative to failing here is handing back a window that is wrong about the stream. A
  /// recoverable error is not on offer and would not be right if it were: the peek surface
  /// returns the *emitter's* error type, so a broken `Cache` would reach the grammar disguised
  /// as a diagnostic about the input.
  ///
  /// `#[inline(never)]` is the measurement above, and both messages stay in their own `#[cold]`
  /// free functions on top of it: this body is generic, so it is one copy per monomorphization,
  /// while [`cache_copy_count_violation`] and [`cache_copy_endpoint_violation`] are one copy
  /// crate-wide — and PEEK_FOOTPRINT is a frame-size law that a callee's frame counts against as
  /// surely as the fill's own.
  #[inline(never)]
  fn assert_cache_copy<W>(
    cache: &Ctx::Cache,
    buf: &Peeked<'_, 'inp, L, W>,
    at: usize,
    reserved: usize,
    staged: usize,
  ) where
    W: Window,
  {
    use crate::cache::PeekedTokenExt as _;

    let copied = buf.len() - at;

    // THE COUNT. The fill left the copy `reserved + want_unspent` slots of room and
    // `Cache::peek`'s own law owes it `min(len(), room)` entries; with `len() == reserved` and
    // `room >= reserved` that is `reserved` exactly, so anything else is the cache disagreeing
    // with the count the fill sized the window from. One compare and a never-taken branch
    // here; the message is built in [`cache_copy_count_violation`], which is where its frame
    // belongs.
    if copied != reserved {
      cache_copy_count_violation(reserved, copied);
    }

    // THE ENDPOINTS. This function is reached only from the exit that rotates, which the fill
    // takes only with `staged > 0` — the one configuration where a clipped copy holes the window
    // rather than shortening it. An overflow-free fill (and a trip, which truncates the staged
    // region away) returns before it, unchecked, exactly as the two resident-head arms do.
    let front = cache.front_span().map(Span::start_ref);
    let whole_run = if copied == 0 {
      // Nothing came back. Sound only if there was nothing to bring.
      front.is_none()
    } else {
      front == Some(buf[at].span().start_ref())
        && cache.back_span().map(Span::end_ref) == Some(buf[buf.len() - 1].span().end_ref())
    };
    if !whole_run {
      cache_copy_endpoint_violation(copied, staged);
    }
  }

  /// Windowed observation in grammar vocabulary, terminal-aware by construction — the
  /// [`peek::<W>`](Self::peek) analogue of [`peek_head_map`](Self::peek_head_map).
  ///
  /// `f` sees the filled window and its value is returned. A window that came back **short**
  /// because the input genuinely ended — or because a non-final [`Partial`](crate::input::Partial)
  /// frontier withheld the rest — is handed to `f` like any other, and is `Ok`. A window cut short
  /// by a **terminal stop** — a resource-limit trip during the fill, or a latched poison boundary
  /// at the cursor — raises the same terminal end-of-input error the `_or_stop` family raises, and
  /// `f` does not run.
  ///
  /// That distinction is the whole of what this adds to [`peek::<W>`](Self::peek), and it is not
  /// something a caller can recover afterwards. `peek` returns `Ok` in both cases, holding a window
  /// of the same length, so a production that decides on the width — *"is the second token a
  /// `(`?"* — reads a halted scanner as a grammar fact and picks the other production. The
  /// diagnostic is not lost either way (see [`peek`](Self::peek)'s Partial-mode section); what is
  /// lost is the *return value's* ability to say which of the two happened, and with a non-fatal
  /// emitter that accepts the diagnostic, the difference is a silently different parse rather than
  /// a stopped one.
  ///
  /// The mark carries the same qualification [`peek_head_map`](Self::peek_head_map)'s does: it is
  /// what an **accepting** emitter earns, after the trip's own diagnostic has gone to it. A fatal
  /// emitter's rejection of that diagnostic still propagates — from the fill here rather than from
  /// a scan — but as *that emitter's* value, converted from the lexer error, so it carries **no**
  /// terminal mark. (A fatal emitter is blind to the difference only on the *emitting* path: at an
  /// already-latched boundary the fill emits nothing, so even there the short window is raised
  /// rather than returned.)
  ///
  /// # The one contract difference from `peek_head_map`
  ///
  /// [`peek_head_map`](Self::peek_head_map) answers `Ok(None)` at a genuine end of input and does
  /// not run `f`; here `f` runs on the window whatever its length, including an empty one. A head
  /// read has two lengths and can lift the empty one into `None`; a `W`-wide window has `W + 1`,
  /// and folding every short one into a single `None` would throw away the tokens that *are* there
  /// — which for a two-token decision is the head the caller already committed to. So the `Option`
  /// belongs to the caller's own projection, not to this return type:
  ///
  /// ```text
  /// // "the second token's kind, if there is a second token"
  /// inp.peek_map::<U2, _, _>(|w| w.iter().nth(1).map(|t| t.token().kind()))
  /// //  Ok(Some(kind)) — a second token
  /// //  Ok(None)       — the input genuinely ends after the head
  /// //  Err(..)        — the scanner stopped while the window was filling
  /// ```
  ///
  /// `f` may also hand the window straight back — `peek_map::<W, _, _>(|w| w)` is exactly
  /// [`peek::<W>`](Self::peek) with the terminal stop moved into the error arm — so nothing the
  /// unmapped form can express is lost. Taking `f` rather than returning the window is what lets
  /// `O` be free of the borrow: the window borrows the cache for as long as it lives, and a
  /// grammar that only wants a kind or a boolean out of it can go on using the input immediately.
  ///
  /// # What is guaranteed to `f`, and the condition on the caller
  ///
  /// `f` runs **exactly once** when it runs at all, is handed the window this call filled, and
  /// nothing is consumed, committed or latched on its behalf. There is one route here and no fast
  /// path, so the two-route reconciliation under *"what is guaranteed identical"* on
  /// [`peek_head_map`](Self::peek_head_map) has no counterpart — but the **second clause of its
  /// condition on the caller** applies verbatim, and for the same mechanism: this call takes
  /// `self.span().end()` before the fill, for the terminal
  /// end-of-input error, and [`peek::<W>`](Self::peek) does not. That is one caller-supplied
  /// `L::Offset::clone` that the unmapped read never runs. An `f` that answers from the *values* of
  /// the window it is handed cannot see it; an `f` that measures the input layer can, and is asking
  /// which primitive this crate reached the window through rather than what the window holds.
  ///
  /// # Footprint and panics
  ///
  /// Rides the same fill as [`peek`](Self::peek), through
  /// [`peek_with_emitter_terminal`](Self::peek_with_emitter_terminal): it reserves the same one
  /// owned window, cache hit or miss, and panics on the same broken-[`Cache`](crate::cache::Cache)
  /// condition. See [`peek`](Self::peek)'s stack-footprint and panics sections.
  #[inline]
  pub fn peek_map<'p, W, O, F>(
    &'p mut self,
    f: F,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    W: Window,
    F: FnOnce(Peeked<'p, 'inp, L, W>) -> O,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    // Hoisted before the fill, exactly as `peek_head_map` hoists it: the committed span does not
    // move under a pure peek, and the window borrow (`peeked` ties to `&'p mut self`) must not
    // overlap the read.
    let end = self.span().end();
    let (peeked, terminal, _emitter) = self.peek_with_emitter_terminal::<W>()?;
    if terminal {
      // The fill met the request short *because the scanner stopped*. Handing the window back
      // here is the whole defect this exists to close: it is byte-for-byte what a genuinely
      // short input produces, so the caller would read a halt as a grammar fact. `f` does not
      // run — a decision taken on this window is a decision taken on a truncated one.
      return Err(UnexpectedEot::eot_of(end).into_terminal().into());
    }
    Ok(f(peeked))
  }

  /// Width-1 head observation in grammar vocabulary, terminal-aware by construction.
  ///
  /// The head-only sibling of [`peek_map`](Self::peek_map), which is the same treatment at an
  /// arbitrary window width.
  ///
  /// `f` sees the head as `Spanned<&Token, &Span>` and its value is returned. `Ok(None)`
  /// is genuine end of input; a **terminal stop** — a resource-limit trip or a latched
  /// poison boundary — raises the same terminal end-of-input error the `_or_stop` family
  /// raises, never a silent `None`. That distinction is the point: a consumer that reads
  /// a halt as "no head here" builds a value out of an input the scanner already gave up
  /// on.
  ///
  /// The mark carries the same qualification the `_or_stop` family's does: it is what an
  /// **accepting** emitter earns, after the trip's own diagnostic has gone to it. A fatal
  /// emitter's rejection of that diagnostic still propagates — from the fill here rather
  /// than from a scan — but as *that emitter's* value, converted from the lexer error, so
  /// it carries **no** terminal mark: no `UnexpectedEot` is built on that path for
  /// [`into_terminal`](crate::error::UnexpectedEnd::into_terminal) to raise a flag on. The
  /// arm of your error type holding a lexer error is what answers for it; see
  /// [`MaybeTerminal`](crate::error::MaybeTerminal#where-the-set-stops-being-closed).
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
  #[cfg_attr(
    feature = "peek",
    doc = " The method form of the free [`peek_kind`](crate::parser::peek_kind). Note the"
  )]
  #[cfg_attr(
    not(feature = "peek"),
    doc = " The method form of the free `peek_kind`. Note the"
  )]
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
