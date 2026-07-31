use generic_arraydeque::GenericArrayDeque;

use super::*;

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
  ///
  /// # Stack footprint: one window, cache hit or miss
  ///
  /// The window this returns is the **only** owned token storage a peek reserves. Its worst case
  /// is the whole array live at once:
  ///
  /// ```text
  /// W::CAPACITY × size_of::<Maybe<CachedToken<&Token, &State, &Span>,
  ///                              CachedToken<Token, State, Span>>>()
  /// ```
  ///
  /// which for every realistic type is `W::CAPACITY × (size_of::<Token>() + size_of::<State>() +
  /// size_of::<Span>())` plus per-entry padding and a discriminant. A **cache miss costs no
  /// second window**: tokens lexed past the cache are staged in this same buffer and rotated into
  /// place, so the miss path reserves exactly what the hit path does. (Through 0.8.0 the miss path
  /// staged them in a separate `W::CAPACITY`-slot array, doubling the figure above.)
  ///
  /// Beyond the window the frame holds one clone of the lexer — `size_of::<L>()`, which contains
  /// `State` — and a small fixed part. Nothing here is heap-allocated, and nothing scales with the
  /// input.
  ///
  /// `Token` and `State` are unconstrained in size, so the bound is linear in both and in the
  /// window width. A grammar carrying a large token payload or a large lexer state pays
  /// `W::CAPACITY` times it for the width it asks for: prefer the narrowest window that decides
  /// the production, and [`peek_kind`](Self::peek_kind) or
  /// [`head_satisfies`](Self::head_satisfies) — which run at `U1` — for a head test.
  ///
  /// # Panics
  ///
  /// On a [`Cache`](crate::cache::Cache) that breaks its own contract, and only then. The fill
  /// reserves the window's cache region from [`Cache::len`](crate::cache::Cache::len) before it
  /// lexes, so a `len` that is not the resident count mis-sizes the room
  /// [`Cache::peek`](crate::cache::Cache::peek) is given. **Every** exit that hands back a window
  /// checks the copy that landed in it — against the room the fill left, and against the cache's
  /// own `front` and `back` — and panics rather than return a window that is wrong about the
  /// stream. Every cache this crate ships conforms, and the
  /// cache conformance kit checks a downstream one.
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
  ///
  /// Reserves the same one owned window as [`peek`](Self::peek), cache hit or miss — see its
  /// stack-footprint section.
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
  ///
  /// Reserves the same one owned window as [`peek`](Self::peek), cache hit or miss — see its
  /// stack-footprint section.
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
      let at = buf.len();
      self.cache.peek::<W>(buf);
      // CACHE_COPY, the cache-hit exit. `clip_ok` is TRUE here and this is the one shape that
      // earns it: the fill reserved a slot per reported resident and found it had fewer slots
      // than residents (that is what `want == 0` means), so `Cache::peek` is *expected* to
      // stop at the window's edge rather than at `back` — and nothing is appended behind the
      // copy, so a tail that stops at capacity is a correct short prefix and not a hole.
      // The licence is narrow, and `assert_cache_copy` is where it is spent: a copy that
      // stops short with room to SPARE is not a capacity clip, it is the over-report
      // shortening the window, and it fails there.
      Self::assert_cache_copy::<W>(self.cache, buf, at, in_cache, true);
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
      let at = buf.len();
      self.cache.peek::<W>(buf);
      // CACHE_COPY, the latched-boundary exit. Nothing is appended behind this copy either, so
      // it carries the same `clip_ok` as the hit exit — inert here, because reaching this point
      // means `want > 0`, i.e. the room left exceeds the reported resident count and no clip
      // should occur at all. The window is short by DESIGN on this path (a terminal stop), which
      // is exactly why it needs the check: shortness carries no information here, so a
      // `Cache::peek` that returned less than it owes would be invisible in the result.
      Self::assert_cache_copy::<W>(self.cache, buf, at, in_cache, true);
      return Ok(self.session.emitter);
    }

    // WHY THE OVERFLOW REGION LIVES IN `buf`.
    //
    // The two halves of a window are produced in the opposite order from the one it must
    // report: the cache region is copied out in one bulk read *after* the fill, while the
    // tokens past the cache are lexed *during* it. That ordering — not a shortage of room —
    // is what a staging area exists to solve. It is solved here by staging those tokens at
    // `buf`'s own tail and, once the cache region has been copied in behind them, rotating
    // them back to the end (see after the loop).
    //
    // Staging in `buf` rather than in a second `W`-slot array is what holds the peek frame to
    // ONE owned window, and it is why this function needs no `unsafe`: `buf` owns each staged
    // entry from the moment it is pushed, so every exit path — success, a withheld frontier, a
    // trip, or a failing return from a fatal emit — frees it exactly once through the deque's
    // own `Drop`, with no partial-initialization bookkeeping to get wrong.
    //
    // The staged region always fits. With `cap = buf.capacity()`, the fill is entered only with
    // `want = cap - buf_len - parked - in_cache` (the `saturating_sub` above returns early at
    // zero), and every token the loop produces decrements `want` and lands in either the cache
    // (`in_cache += 1`) or the staged region. So at every point
    // `buf_len + staged + parked + in_cache <= cap`, and the per-push assertion below pins the
    // same bound locally. Note what that accounting reserves: the cache region's room as much
    // as the staged region's, sized from `Cache::len()` before a single token is staged. A
    // `Cache` whose `len` is not its resident count therefore mis-sizes the copy that comes
    // after the loop — see CACHE_COPY there for the check that refuses to rotate on one.
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
                // Cache full: stage the overflow token at the tail of the output buffer. The
                // capacity argument above proves the push is always accepted; assert it in debug
                // builds so a future change to the accounting cannot silently drop a token.
                let refused = buf.push_back(Maybe::Owned(ct));
                debug_assert!(
                  refused.is_none(),
                  "peek staged an overflow token past the window capacity"
                );
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
      *terminal = true;
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
    if Self::can_park() {
      if let Some(parked) = self.pending.as_ref() {
        buf.push_back(Maybe::Ref(parked.as_ref()));
      }
    }
    let cache_copy_at = buf.len();
    debug_assert!(
      cache_copy_at == buf_len + staged + parked,
      "the fill pushed something it did not account for before the cache copy"
    );
    self.cache.peek::<W>(buf);

    // CACHE_COPY, the cache-miss exit — the one exit where the copy is NOT the window's tail.
    //
    // `clip_ok` is FALSE here, for two independent reasons and either would do:
    //
    //   * the staged overflow region is rotated in BEHIND this copy, so a copy that stopped
    //     short of `back` would be closed up by tokens that belong *after* the residents the
    //     clip dropped — a hole, not a short window;
    //   * the fill reserved a window slot per reported resident before it lexed, and the loop
    //     never spends more than `want`, so at this point the room left is `want_unspent +
    //     in_cache >= in_cache` by the fill's own arithmetic. There is room for the whole run
    //     by construction, and a clip is a defect even where nothing follows it.
    //
    // The second reason is why `staged > 0` is not what is passed: it would be enough, but it
    // would make an accounting slip in the fill look like a licensed clip instead of the bug
    // it is.
    Self::assert_cache_copy::<W>(self.cache, buf, cache_copy_at, in_cache, false);

    if tripped {
      return Ok(self.session.emitter);
    }

    if staged > 0 {
      // Restore stream order. The staged tokens were appended *before* the front-of-stream
      // region, so the region this fill owns currently reads `[staged…][parked?][cache…]` where
      // the window must read `[parked?][cache…][staged…]`. One left-rotation by `staged` over
      // that region (`buf_len..`) is exactly that permutation, and it permutes values inside the
      // deque — nothing is copied out, so no entry can be leaked or dropped twice. Entries the
      // caller passed in (`..buf_len`) are excluded, so a prefilled buffer keeps its order too.
      buf.make_contiguous()[buf_len..].rotate_left(staged);
    }

    #[cfg(debug_assertions)]
    {
      if want == 0 {
        debug_assert!(
          exp == (in_cache - initial_in_cache) + staged,
          "expected peeked token count mismatch"
        );
      }
    }

    Ok(self.session.emitter)
  }

  /// CACHE_COPY — the window invariant, stated once and checked on **every** exit that hands a
  /// window back.
  ///
  /// # The invariant
  ///
  /// > A peek window is a **contiguous prefix of the token stream at the cursor**. The cache
  /// > region inside it — the one region the fill does not place itself — is therefore either
  /// > the cache's **whole resident run**, `front` through `back`, or a run the **window's own
  /// > capacity** cut short with **nothing behind it**. There is no third case, and "short" is
  /// > never an excuse: a copy that stops with slots to spare has lost tokens the fill was
  /// > entitled to.
  ///
  /// Both halves matter and they fail differently, which is why one check cannot replace the
  /// other:
  ///
  /// - a copy clipped **mid-run with something behind it** is a **HOLE** — the region after it
  ///   belongs after the residents the clip dropped, so the window reports a token at a position
  ///   where the next consume serves a different one;
  /// - a copy that stops **with room to spare** is a **SHORT WINDOW** — correct as far as it
  ///   goes, but a caller is entitled to read a window shorter than it asked for as end of
  ///   input while the input still has more.
  ///
  /// # Why a caller-facing check exists at all
  ///
  /// `Cache::len` is load-bearing: the fill reserves the window's cache region from it *before*
  /// it lexes, spends the rest of the window on tokens lexed past the cache, and only then calls
  /// [`Cache::peek`](crate::cache::Cache::peek) into the room that is left. A `len` that is not
  /// the resident count therefore mis-sizes this copy in one direction or the other, and both
  /// directions are above.
  ///
  /// # Why the guards are shaped the way they are
  ///
  /// Every quantity a check is *gated* on here is one the **fill** computes — `at`, `room`,
  /// `copied`, `clip_ok` — never one the cache reports. That is deliberate, and it is the
  /// correction of a real defect: two earlier revisions of this check each gated on a
  /// cache-supplied value, and each was blind exactly where the lie was largest.
  ///
  ///  1. The first was a count identity, `buf_len + staged + parked + in_cache == buf.len()`.
  ///     An under-report is subtracted from `in_cache` and added right back as `staged`, so
  ///     the identity held on **both sides** of the lie it was meant to catch (residents
  ///     `[a,b,c]`, `len()` 2, a `U4` window: `0 + 2 + 0 + 2 == 4`, window `[a,b,d,e]` for a
  ///     stream that reads `[a,b,c,d]`). Self-cancelling.
  ///  2. The second replaced it with the endpoint witness below, but wrote the precondition as
  ///     `in_cache > 0` — the untrusted value again. A cache under-reporting to **zero** skipped
  ///     the witness by that precondition and passed the surviving count check trivially as
  ///     `0 == 0`.
  ///
  /// A precondition is an escape hatch unless it is itself verified. So `reserved` appears here
  /// only on the *expected* side of a comparison, where a lie shows up as a mismatch; it never
  /// decides whether a comparison runs.
  ///
  /// # Parameters
  ///
  /// - `at` — where the copy began: `buf.len()` immediately before `Cache::peek`.
  /// - `reserved` — the resident count the fill believes the cache has: `Cache::len()` read
  ///   before the fill, plus every token the fill then appended. Untrusted.
  /// - `clip_ok` — whether the window's own capacity may legitimately cut this copy short of
  ///   `back`. **True only where the copy is the window's tail**, so a clip shortens rather
  ///   than holes. It is *not* a licence to come back short: see `expect` below.
  ///
  /// # Panics
  ///
  /// On a `Cache` that breaks the contract above, in **every build** — the posture
  /// [`InputRef::park`](super::InputRef) takes for a `RETAINS_FRONT` refusal and
  /// [`Sink::cst_start`](crate::cst::Sink::cst_start) takes for a node kind. The violation is in
  /// caller-supplied trait code, no caller can repair its own `Cache` mid-parse, and the
  /// alternative to failing here is handing back a window that is wrong about the stream. A
  /// recoverable error is not on offer and would not be right if it were: the peek surface
  /// returns the *emitter's* error type, so a broken `Cache` would reach the grammar disguised
  /// as a diagnostic about the input.
  #[inline]
  fn assert_cache_copy<W>(
    cache: &Ctx::Cache,
    buf: &Peeked<'_, 'inp, L, W>,
    at: usize,
    reserved: usize,
    clip_ok: bool,
  ) where
    W: Window,
  {
    use crate::cache::PeekedTokenExt as _;

    let room = buf.capacity() - at;
    let copied = buf.len() - at;

    // THE COUNT — what `Cache::peek`'s own law owes, computed from two fill-side facts and the
    // reported length. This is the half that catches an OVER-report: a cache claiming residents
    // it does not have makes the fill lex too few tokens to fill the window, and the copy then
    // comes back short of the room waiting for it. Where a clip is licensed, `min` is what
    // distinguishes the licensed clip (`copied == room`, the window full) from that short copy
    // (`copied < room`, slots to spare) — the licence is spent here, not skipped.
    let expect = if clip_ok {
      reserved.min(room)
    } else {
      reserved
    };
    assert!(
      copied == expect,
      "`Cache` contract violation: the fill counted {reserved} resident entr(ies) from \
       `Cache::len`, left `Cache::peek` room for {room} of them, and so is owed {expect} — \
       and `Cache::peek` appended {copied}. `len` must be exactly the resident count and \
       `peek` must append exactly `min(len(), the buffer's remaining capacity)`",
    );

    // THE ENDPOINTS — the resident run's own witness, read from `front`/`back` rather than from
    // `len`, so an inexact `len` cannot both cause the fault and hide it. This is the half that
    // catches an UNDER-report, including one all the way to zero: the count above is satisfied
    // by `0 == 0` there, and only the cache's own front can say that a run exists which the copy
    // did not bring back.
    //
    // `back` is consulted lazily, and only where it is load-bearing: a licensed clip that filled
    // the window is *expected* to stop before `back`, so the comparison would be wrong there as
    // well as wasted. That short-circuit is what keeps the cache-hit path — the width-1 head
    // tests a grammar runs per token — down to one endpoint read.
    //
    // Compiled as `debug_assert!`, not `assert!` like THE COUNT above. What this witness guards
    // is a LOGIC invariant, not a soundness one: this module now contains zero `unsafe`, so a
    // `Cache` that fails it cannot corrupt memory — it can only hand back a window that
    // misdescribes the stream — and THE COUNT still stands guard in every build for the
    // OVER-report direction it alone can see. The UNDER-report direction, the one only this
    // witness catches, is the expensive one to keep checking on every fill: two ring indexes, a
    // `Maybe` discriminant, and an `Option<&Span>` compare price out at 52–55 instructions
    // retired on the cache-hit exit — against a base fill there of roughly 52, so this one check
    // more than doubles the one path a grammar runs per token — and 67–70 on a cache-miss fill,
    // where the fill's own lexing already dwarfs it. A `Cache` that breaks this half of the
    // contract still fails fast, just in debug builds and under test, rather than on every
    // release-mode token a grammar peeks.
    let front = cache.front_span().map(Span::start_ref);
    let whole_run = if copied == 0 {
      // Nothing came back. Sound only if there was nothing to bring — or, under a licensed
      // clip, if the window had no room left to bring it into.
      front.is_none() || (clip_ok && room == 0)
    } else {
      front == Some(buf[at].span().start_ref())
        && ((clip_ok && copied == room)
          || cache.back_span().map(Span::end_ref) == Some(buf[buf.len() - 1].span().end_ref()))
    };
    debug_assert!(
      whole_run,
      "`Cache` contract violation: `Cache::peek` did not copy the cache's whole resident \
       run. The {copied} entr(ies) it appended into {room} slot(s) of room do not span the \
       cache from `front` to `back`. A copy may stop short of `back` only where the \
       window's own capacity stopped it AND nothing is appended behind it in the window; \
       neither excuse holds here. That is what an inexact `Cache::len` does to this copy: \
       the fill reserves the room from `len`, so a `len` below the resident count clips the \
       copy mid-run and the window would report a token the next consume will not serve",
    );
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
