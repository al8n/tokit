//! The shared scanner behind [`skip_while`](InputRef::skip_while) and the
//! `sync_to`/`sync_through`/`sync_balanced` family: **one** loop over the token stream — cached
//! tokens and freshly-lexed ones alike — parameterized by a [`ScanMode`] policy that decides how
//! the token the scan stops on settles, what end of input does, and whether the tokens it skipped
//! are diagnosed.
//!
//! # One loop, because the cache is invisible
//!
//! Whether a token had already been peeked into the cache is an optimization the caller cannot
//! see: every observable of a scan — its return, the committed position and lexer state, the
//! diagnostics it emits, the resume cursor, the poison boundary, the dedup watermark — is a
//! function of the token stream alone, never of how much of it had been prefetched. Each scanner
//! used to keep that promise by *agreeing*: a cache-drain prologue and a lexing loop each
//! implemented skip-and-stop, and each settled the stopping token its own way. Nothing forced the
//! two to agree, and they repeatedly did not.
//!
//! So there is now exactly one implementation. The loop takes its next token from the front of the
//! stream while one is there and from the lexer once there is not ([`Fetched`], carried as the crate's
//! [`CachedToken`] — a lexed token plus the state that produced it — whichever way it arrived).
//! That fetch is the *whole* of the difference: the predicate is evaluated at one site, the
//! skip-and-report is one method, and the stop settles through one [`ScanMode`] hook that cannot
//! even tell where the token came from. A cached/uncached divergence is no longer a bug to be
//! caught by a test — it has nowhere to live.
//!
//! # Four modes, three decisions
//!
//! Every scanner in the crate skips a run of tokens and then stops on one. They differ in only
//! three decisions, and all three are the mode's:
//!
//! - **how the stopping token settles** — a `to`-shaped scan stops *before* it, leaving it
//!   unconsumed; a `through` scan consumes it;
//! - **what end of input does** — a `to`-shaped scan commits at the lexer's end; a `through` scan
//!   rewinds the full pre-call state, so a no-match run leaves no trace;
//! - **whether each skipped token is reported** — the to/through family diagnoses per skipped
//!   token; the balanced scan describes the whole hole with one diagnostic instead; and
//!   [`skip_while`](InputRef::skip_while) reports nothing at all — its skipped tokens are trivia,
//!   which is *expected*, not garbage.
//!
//! | mode             | drives                      | the stop settles | end of input | reports |
//! |------------------|-----------------------------|------------------|--------------|---------|
//! | [`SyncTo`]       | `sync_to`                   | unconsumed       | commit       | yes     |
//! | [`SyncThrough`]  | `sync_through`              | consumed         | rewind       | yes     |
//! | [`SyncBalanced`] | `sync_balanced`             | unconsumed       | rewind       | no      |
//! | [`SkipWhile`]    | `skip_while` (and `padded`) | unconsumed       | commit       | no      |
//!
//! The sync family stops on the token its predicate *matches*; `skip_while` stops on the first one
//! its predicate *rejects*. That polarity is one negation at the call site, not a second scanner:
//! both are "skip a run of tokens, then stop on one", so the trivia path and the recovery path are
//! the same loop and cannot drift apart.
//!
//! Everything else — the poison-boundary short-circuit, the dedup watermark lifted through
//! [`scan_with`](InputRef::scan_with), and the trip-commit at the durable frontier — is identical,
//! so it lives here once and the contracts documented on the public methods are structural instead
//! of re-implemented per method.
//!
//! # An unconsumed token lives at the front of the stream
//!
//! That is the invariant a `to`-shaped stop settles on, and it is not new: a token whose predicate
//! declined it is exactly what [`try_expect`](InputRef::try_expect) puts back to the front, and
//! the front is the one place [`cursor`](InputRef::cursor) reads. The old scanners broke it —
//! they threw a *lexed* stopping token away and let the caller re-lex it, while keeping a *cached*
//! one — so the same call left a different resume cursor (and returned a different zero-skip
//! [`Hole`](super::Hole)) depending on how deep the caller had peeked. Settling through
//! [`InputRef::unconsume`] restores the invariant on *both* origins, so the cursor after a stop is
//! the stopping token's start no matter who lexed it — and, under a cache that retains nothing, in
//! the parked front slot `Input` keeps outside the cache for exactly this promise.

use super::*;

/// The normalized outcome of [`skip_until`](InputRef::skip_until), across every mode and entry
/// point. The caller maps it to its own return shape; the position is already settled per policy
/// by the time it is handed back.
pub(super) enum Scanned<'inp, L>
where
  L: Lexer<'inp>,
{
  /// The scan stopped on a token. A `through` policy consumes it and carries it here; a
  /// `to`-shaped policy leaves it unconsumed at the front of the stream and carries `None` (its caller
  /// peeks it straight back out, or — for `skip_while` — pays it no further attention).
  Found(Option<Spanned<L::Token, L::Span>>),
  /// End of input or a poison trip — nothing stopped the scan. The position is already settled per
  /// policy (committed at the frontier/end, or rewound to the pre-call snapshot), so the caller
  /// only produces its exhausted return.
  Exhausted,
}

/// One token under decision: the token, its span, and the lexer state that produced it — the
/// crate's [`CachedToken`], which is precisely that triple, whichever origin it arrived from.
///
/// The [`Origin`] rides along for exactly one reason: the cache's push history. Putting a *popped*
/// token back is a no-op on that history, while a *lexed* or *unparked* one becomes a new cache
/// entry; see [`InputRef::hold_front`], the single place that knows.
///
/// Normalizing both origins into this one carrier is what makes the rest of the loop origin-blind:
/// the predicate, the skip-and-report, and the stop settle all take a `Fetched` and cannot tell a
/// drained cache from a fresh lex.
///
/// Visible to the enclosing module only because [`ScanMode`] is (its hooks take one); both fields
/// stay private to this one, so nothing outside the scanner can build a `Fetched` and mislabel its
/// origin.
pub(super) struct Fetched<'inp, L>
where
  L: Lexer<'inp>,
{
  tok: CachedTokenOf<'inp, L>,
  origin: Origin,
}

/// The pre-call snapshot a rewinding end-of-input arm restores. Captured *before* the scan runs so
/// the rewind restores the FULL pre-call state — span, lexer state, emission mark, and dedup
/// watermark — leaving a no-match run to end of input with no trace, including across a prefilled
/// cache the loop drained (see [`sync_through`](InputRef::sync_through)).
pub(super) struct ThroughEntry<Span, State, Offset> {
  span: Span,
  state: State,
  mark: u64,
  error_end: Offset,
}

impl<Span, State, Offset> ThroughEntry<Span, State, Offset> {
  /// Bundles the four facts the end-of-input rewind restores.
  #[inline(always)]
  pub(super) const fn new(span: Span, state: State, mark: u64, error_end: Offset) -> Self {
    Self {
      span,
      state,
      mark,
      error_end,
    }
  }
}

/// How a scan settles the three decisions that separate its entry points (the table in the
/// [module docs](self)).
///
/// [`SyncTo`] and [`SkipWhile`] stop before the token (committing at the frontier, leaving it
/// unconsumed at the front of the stream) and, at end of input, commit at the lexer's end; [`SyncThrough`]
/// consumes the token and, at end of input, rewinds the full pre-call state; [`SyncBalanced`] takes
/// one settle from each. All four are zero-sized; the pred/exp closures and the pre-call snapshot
/// are threaded through [`skip_until`](InputRef::skip_until) rather than held here.
///
/// No hook is told where its token came from — that is the point. Each is handed the same
/// [`Fetched`] carrier whether the loop popped it off the cache or lexed it, so a settle cannot be
/// written to depend on the cache even by accident.
pub(super) trait ScanMode<'inp, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  /// The pre-call snapshot the end-of-input arm needs: `()` for the committing modes, a
  /// [`ThroughEntry`] for the rewinding ones.
  type Snapshot;

  /// Whether the scan reports each skipped token through `emit_unexpected_token`. The to/through
  /// family diagnoses per skipped token; the balanced scan suppresses the per-token reports because
  /// one skipped-region diagnostic describes the whole hole (see
  /// [`sync_balanced`](InputRef::sync_balanced)); [`skip_while`](InputRef::skip_while) suppresses
  /// them because its skipped tokens are expected trivia, not garbage.
  const REPORT_SKIPPED: bool;

  /// Settle the input on the token the scan stopped on, and produce the carried token. A
  /// `to`-shaped mode commits at `frontier` (the end of the last skipped token, i.e. before the
  /// stop), leaves the token unconsumed at the front of the stream, and returns `None`; a `through` mode
  /// consumes it (commits at its span, adopting the state that produced it) and returns
  /// `Some(tok)`.
  fn on_stop(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    frontier: AtFrontier<L::Span, L::State>,
    stopper: Fetched<'inp, L>,
  ) -> Option<Spanned<L::Token, L::Span>>;

  /// Settle the input at end of input (nothing stopped the scan). The committing modes commit at
  /// the lexer's end; the rewinding ones restore span, lexer state, dedup watermark, and emissions
  /// from `snapshot`.
  fn on_eof(ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, lexer: L, snapshot: Self::Snapshot);

  /// RELEASE_CENSUS — settle the pre-call snapshot on an exit that **keeps** the scan's
  /// progress (a stop, a trip, a boundary drain, either fatal propagation): the snapshot will
  /// never be restored, so a rewinding mode [`release`](Emitter::release)s the emitter mark it
  /// holds — the keep-dual of the rewind its [`on_eof`] performs — while the committing modes
  /// hold no snapshot and do nothing. Called after the exit's own settle, so a mark is released
  /// only once the kept progress (position commit, stop settle, fatal report) is fully in
  /// place; [`skip_until`](InputRef::skip_until) pairs every exit with exactly one of this and
  /// [`on_eof`], and the census locks the count.
  ///
  /// [`on_eof`]: ScanMode::on_eof
  fn on_commit(ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, snapshot: Self::Snapshot);
}

/// Stop *before* the sync token: commit at the frontier on a match, commit at the lexer's end at
/// end of input. Drives `sync_to`.
pub(super) struct SyncTo;

impl<'inp, L, Ctx, Lang, Cmpl> ScanMode<'inp, L, Ctx, Lang, Cmpl> for SyncTo
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  type Snapshot = ();

  const REPORT_SKIPPED: bool = true;

  #[inline(always)]
  fn on_stop(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    frontier: AtFrontier<L::Span, L::State>,
    stopper: Fetched<'inp, L>,
  ) -> Option<Spanned<L::Token, L::Span>> {
    // Leave the token unconsumed — which in this crate means AT THE FRONT OF THE STREAM, the home
    // of every lexed-but-not-consumed token (`try_expect`'s decline puts one back there too) and
    // the one place `cursor()` reads. The caller peeks it straight back out. Doing this for a token
    // the loop LEXED, and not only for one it popped, is what makes the resume cursor after a stop
    // a fact about the stream instead of about the caller's lookahead depth.
    ir.unconsume(stopper);
    // Commit before the stop: the end of the last skipped token, with the state that produced it.
    ir.commit_at(frontier);
    None
  }

  #[inline(always)]
  fn on_eof(ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, lexer: L, _snapshot: ()) {
    // Nothing stopped the scan: commit the whole skipped run at the lexer's end. `sync_to` reports
    // as it goes and keeps that progress, so end of input is not a rewinding failure here.
    ir.set_span_after_consume(lexer.span().into());
    *ir.state = lexer.into_state();
  }

  #[inline(always)]
  fn on_commit(_ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, _snapshot: ()) {
    // A committing mode snapshots nothing, so a kept exit has nothing to release.
  }
}

/// Consume the sync token: commit at its span on a match, rewind the full pre-call state at end
/// of input. Drives `sync_through` and `sync_through_then_peek`.
pub(super) struct SyncThrough;

impl<'inp, L, Ctx, Lang, Cmpl> ScanMode<'inp, L, Ctx, Lang, Cmpl> for SyncThrough
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  type Snapshot = ThroughEntry<L::Span, L::State, L::Offset>;

  const REPORT_SKIPPED: bool = true;

  #[inline(always)]
  fn on_stop(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    _frontier: AtFrontier<L::Span, L::State>,
    stopper: Fetched<'inp, L>,
  ) -> Option<Spanned<L::Token, L::Span>> {
    // Consume the match: commit at its span, adopting the state that produced it. This is
    // `consume_cached_one`'s body over the same carrier — and it is the same two lines whether the
    // token was popped off the cache or lexed a moment ago, because a `CachedToken` carries the
    // post-token state either way.
    let (tok, state) = stopper.tok.into_components();
    ir.commit_token(tok.data(), tok.span_ref());
    *ir.state = state;
    Some(tok)
  }

  #[inline(always)]
  fn on_eof(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    _lexer: L,
    snapshot: ThroughEntry<L::Span, L::State, L::Offset>,
  ) {
    // No match reached the end of input: this path commits no progress, so it rewinds the FULL
    // pre-call state — the drained cache prefix included. Restore span/state, restore the dedup
    // watermark, and unwind every emission this call made. The loop drained the cache, so
    // restoring span/state lands the cursor exactly at the pre-call position (with nothing cached,
    // the cursor follows span.end). Restoring the watermark keeps a rewound lexer error
    // re-emittable, so a later genuine consume reports it exactly once instead of deduplicating it
    // silently away.
    ir.set_span((&snapshot.span).into());
    *ir.state = snapshot.state;
    *ir.emitted_error_end = snapshot.error_end;
    let cursor = ir.cursor().clone();
    ir.emitter().rewind(&cursor, snapshot.mark);
  }

  #[inline(always)]
  fn on_commit(ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, snapshot: Self::Snapshot) {
    // The scan kept its progress, so the entry snapshot will never be restored: release the
    // emitter mark it captured — the keep-dual of `on_eof`'s rewind. Advisory (a no-op for
    // every built-in emitter); a mark-keyed emitter reclaims its row here instead of
    // stranding one per successful sync.
    ir.emitter().release(snapshot.mark);
  }
}

/// Stop *before* the sync token like [`SyncTo`], but rewind the full pre-call state at end of
/// input like [`SyncThrough`], and report no per-token diagnostics — the hole diagnostic that
/// [`sync_balanced`](InputRef::sync_balanced) emits on success describes the whole skipped
/// region. Composed from the other two modes' settles.
pub(super) struct SyncBalanced;

impl<'inp, L, Ctx, Lang, Cmpl> ScanMode<'inp, L, Ctx, Lang, Cmpl> for SyncBalanced
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  type Snapshot = ThroughEntry<L::Span, L::State, L::Offset>;

  const REPORT_SKIPPED: bool = false;

  #[inline(always)]
  fn on_stop(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    frontier: AtFrontier<L::Span, L::State>,
    stopper: Fetched<'inp, L>,
  ) -> Option<Spanned<L::Token, L::Span>> {
    // Stop before the sync point, exactly as `sync_to` does — which is also what places the
    // zero-skip hole: `sync_balanced` anchors it at `cursor()`, and the cursor is the match's start
    // because the match is left at the front of the stream here, cached, parked or lexed.
    <SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::on_stop(ir, frontier, stopper)
  }

  #[inline(always)]
  fn on_eof(ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, lexer: L, snapshot: Self::Snapshot) {
    // A failed balanced sync leaves no trace, exactly as `sync_through`'s no-match exit.
    <SyncThrough as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::on_eof(ir, lexer, snapshot)
  }

  #[inline(always)]
  fn on_commit(ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, snapshot: Self::Snapshot) {
    // A kept balanced sync releases its entry mark, exactly as `sync_through`'s keep exits.
    <SyncThrough as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::on_commit(ir, snapshot)
  }
}

/// Stop *before* the token that ends the run and commit at the lexer's end at end of input,
/// exactly as [`SyncTo`] does — but diagnose nothing. Drives
/// [`skip_while`](InputRef::skip_while), and through it the `padded` combinators.
///
/// The suppressed report is the whole of what separates the trivia path from the recovery path: a
/// `skip_while` skips tokens that are *expected* (whitespace, comments), so no unexpected-token
/// diagnostic is built for them — `REPORT_SKIPPED` is `false`, which erases the report at
/// monomorphization and never calls the `exp` closure. Genuine lexer errors crossed on the way are
/// still emitted, deduplicated, through [`scan_with`](InputRef::scan_with) exactly as everywhere
/// else.
///
/// The settle is `SyncTo`'s, verbatim, and that is the point: the token that stopped a trivia skip
/// is left where every unconsumed token in this crate lives — the front of the stream — so
/// `cursor()` after a `skip_while` (and therefore after a `padded`) is a fact about the token
/// stream, not about how deep the caller had peeked.
pub(super) struct SkipWhile;

impl<'inp, L, Ctx, Lang, Cmpl> ScanMode<'inp, L, Ctx, Lang, Cmpl> for SkipWhile
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  type Snapshot = ();

  const REPORT_SKIPPED: bool = false;

  #[inline(always)]
  fn on_stop(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    frontier: AtFrontier<L::Span, L::State>,
    stopper: Fetched<'inp, L>,
  ) -> Option<Spanned<L::Token, L::Span>> {
    <SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::on_stop(ir, frontier, stopper)
  }

  #[inline(always)]
  fn on_eof(ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, lexer: L, snapshot: ()) {
    // Everything from the cursor to the end of input matched, and was skipped: keep that progress,
    // at the lexer's end.
    <SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::on_eof(ir, lexer, snapshot)
  }

  #[inline(always)]
  fn on_commit(ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, snapshot: ()) {
    // Nothing snapshotted, nothing to release — and this empty body is what keeps the trivia
    // hot path's kept exits free of any per-call work.
    <SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::on_commit(ir, snapshot)
  }
}

impl<'inp, L, Ctx, Lang, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  /// **The** scanner: skip tokens — diagnosing each as unexpected if the mode says so — until
  /// `pred` stops the scan or the input is exhausted, then settle per the [`ScanMode`] `M`.
  ///
  /// The loop takes each token from the front of the stream while one is there and from the lexer
  /// once there is not — and that is the only thing the two origins change. `pred` is evaluated at a single
  /// site, exactly once per token, so a stateful `FnMut` cannot tell a drained cache from a fresh
  /// lex; the skip-and-report is [`skip_and_report`](Self::skip_and_report), one method for both;
  /// and the stopping token settles through [`ScanMode::on_stop`], which is handed the same carrier
  /// either way ([`Scanned::Found`]). A limit trip commits the skipped prefix at the durable
  /// frontier and end of input settles via [`ScanMode::on_eof`] (both [`Scanned::Exhausted`]).
  ///
  /// `pred` names the token the scan **stops on**. The sync family hands over its sync predicate
  /// directly; [`skip_while`](Self::skip_while) hands over the negation of its own — it skips
  /// exactly the tokens a sync would not, and stops on exactly the ones a sync would skip. That
  /// polarity belongs to the caller, so both drive this one loop.
  ///
  /// # The frontier is the scan's uncommitted position
  ///
  /// Nothing is written back to the input while the loop runs: each skipped token settles behind
  /// the [`AtFrontier`] frontier — its span and the state that produced it, arriving with the token
  /// from the cache or read off the lexer — and every stop writes the input's position *from
  /// there* ([`commit_at`](Self::commit_at)). So the committed position after a scan is a function
  /// of the tokens the loop skipped, never of where they came from, and the lexer that takes over
  /// when the cache runs dry is built from that same frontier (its state, at its end — precisely
  /// where the drained cache left the lex position).
  ///
  /// # The fatal exit commits, so the cache stays invisible
  ///
  /// A fatal rejection of a skipped token's diagnostic commits that token before propagating — the
  /// family's fatal-exit discipline. It holds identically on both origins because there is only one
  /// path: [`skip_and_report`](Self::skip_and_report) settles the token behind the frontier
  /// *before* the report's verdict is honoured, and this loop commits at that frontier on the way
  /// out. Returning without the commit would leave the reported token unconsumed here and consumed
  /// there, so a recovery that retries would duplicate diagnostics — or spin — on exactly the runs
  /// where the token had not been prefetched.
  ///
  /// # Every exit settles the pre-call snapshot exactly once
  ///
  /// The loop has six exits. Five keep the scan's progress — the boundary drain, a fatal
  /// lexer-error rejection propagating out of [`scan_with`](Self::scan_with), a limit trip, a
  /// stop, and a fatal skipped-token report — and each settles the mode's snapshot through
  /// [`ScanMode::on_commit`] after its own position settle (a rewinding mode releases the
  /// emitter mark it captured at entry; the committing modes hold none). The sixth is end of
  /// input, where [`ScanMode::on_eof`] spends the snapshot instead — for the rewinding modes,
  /// by rewinding to its mark. One snapshot, one settle, per call (RELEASE_CENSUS).
  ///
  /// # `inline(always)` is load-bearing, because one of the four modes is a hot path
  ///
  /// [`SkipWhile`] puts this loop on the trivia path, where it runs per token. Left to the
  /// inliner's own judgement (a plain `#[inline]`) the loop stays out of line at its `skip_while`
  /// call site, and the lexer — which lives in an `Option`, built the moment the cache runs dry —
  /// then cannot be scalar-replaced, so it is re-loaded from the stack on every token. Forcing the
  /// inline restores it to registers and is worth ~6% of `skip_trivia_next` (`benches/input_scan`).
  /// The cold modes pay for that in code size at their call sites, which is the right way round:
  /// the alternative is a second, hand-tuned loop for the trivia path, and a second loop is exactly
  /// the thing whose disagreement with this one produced the defects this scanner exists to make
  /// impossible.
  #[inline(always)]
  pub(super) fn skip_until<M, F, Exp>(
    &mut self,
    mut pred: F,
    mut exp: Exp,
    snapshot: M::Snapshot,
  ) -> Result<Scanned<'inp, L>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    M: ScanMode<'inp, L, Ctx, Lang, Cmpl>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
    Exp: FnMut() -> Option<Expected<'inp, <L::Token as Token<'inp>>::Kind>>,
  {
    // The scan's uncommitted position: the pre-call span/state, then the end of each token the loop
    // settles behind it. A trip latches here, and every stop that keeps the loop's progress commits
    // here.
    let mut frontier = AtFrontier {
      span: self.span.clone(),
      state: self.state.clone(),
    };
    // The lexer, built the moment the cache runs out — under the frontier's state and at its end,
    // which is exactly where the drained cache left the lex position. A call answered entirely out
    // of the cache never builds one.
    let mut lexing: Option<Resume<L, L::Offset>> = None;

    loop {
      // ── The one place the two origins differ: where the next token comes from ──
      // A retained token arrives with the state that lexed it. One popped off the CACHE was
      // already counted by the peek that cached it; one taken out of the parked slot never was,
      // so putting it back is a genuinely new entry for a cache that now has room.
      let fetched = if Self::can_park() && self.pending.is_some() {
        Fetched {
          tok: self.take_front_parked().expect("probed just above"),
          origin: Origin::Parked,
        }
      } else if let Some(tok) = self.take_front() {
        // Reached with the parked slot provably empty, so this is the cache pop.
        Fetched {
          tok,
          origin: Origin::Cache,
        }
      } else {
        if lexing.is_none() {
          let at = frontier.span.end_ref().clone();
          // A sticky limit trip latches a poison boundary: once the lex position has reached the
          // durable frontier there is no token left to scan. Commit what the loop already
          // skipped — real progress — and yield the exhausted outcome without rebuilding a lexer.
          if self.reached_boundary(&at) {
            self.commit_at(frontier);
            M::on_commit(self, snapshot);
            return Ok(Scanned::Exhausted);
          }
          lexing = Some(self.resume_at_frontier(&frontier));
        }
        let resume = lexing.as_mut().expect("the lexer is built just above");
        // `scan_with` centralizes the poison latch, the dedup watermark, the partial-input
        // frontier rules, and the fatal-emit discipline, handing back only the events this loop
        // must decide.
        match self.scan_with(resume.parts_mut(), &frontier) {
          Ok(Scan::Token(tok)) => Fetched {
            tok: CachedToken::new(tok, resume.lexer().state().clone()),
            origin: Origin::Lexer,
          },
          Ok(Scan::Tripped) => {
            // Commit the skipped prefix at the durable frontier — the end of the last skipped
            // token — so a later scan yields the poisoned outcome there instead of stranding
            // those tokens at the cursor. That commit is real progress, so any diagnostics made
            // over it persist.
            self.commit_at(frontier);
            M::on_commit(self, snapshot);
            return Ok(Scanned::Exhausted);
          }
          Ok(Scan::Eof) => {
            let lexer = lexing
              .take()
              .expect("the lexer is built just above")
              .into_lexer();
            M::on_eof(self, lexer, snapshot);
            return Ok(Scanned::Exhausted);
          }
          Err(e) => {
            // A fatal rejection of a crossed lexer error's diagnostic: `settle_fatal` already
            // committed the position inside `scan_with`, so this exit KEEPS the scan's progress
            // and the entry snapshot will never be restored — settle it as kept. The frontier
            // is deliberately not committed here, exactly as before: the rejected item is an
            // error, not a skipped token.
            M::on_commit(self, snapshot);
            return Err(e);
          }
        }
      };

      // ── One decision, one report, one settle — all of it blind to the origin ──
      // `pred` sees each token EXACTLY once, at this single site.
      if pred(fetched.tok.token()) {
        let carried = M::on_stop(self, frontier, fetched);
        M::on_commit(self, snapshot);
        return Ok(Scanned::Found(carried));
      }

      if let Err(e) = self.skip_and_report::<M, _>(fetched, &mut frontier, &mut exp) {
        // The family's fatal-exit discipline: the token that trips a fatal emitter is committed,
        // and the error propagates. The commit lands at the frontier — the skipped token's end,
        // with the state that produced it — because `skip_and_report` settled it there before
        // honouring the verdict. It also carries the prefix this loop already diagnosed, so nothing
        // already reported is left to be reported again.
        self.commit_at(frontier);
        M::on_commit(self, snapshot);
        return Err(e);
      }
    }
  }

  /// **The** skip-and-report path: settle a token the predicate did not stop on behind the
  /// frontier and — for the modes that diagnose each skipped token — report it as unexpected.
  ///
  /// Cached tokens and freshly-lexed ones reach this by the same call, carrying the same
  /// [`CachedToken`], so the settle and the report cannot drift apart: the crate has one answer to
  /// "skip a token and report it", not one per origin.
  ///
  /// The token settles behind the frontier **before** the report's verdict is honoured, so both
  /// outcomes leave it behind the frontier and the caller's fatal exit commits it — the family's
  /// trip-commit, on either origin. The quiet modes report nothing (a balanced scan's whole region
  /// is described by one hole diagnostic; a `skip_while`'s skipped tokens are expected trivia), so
  /// under them the diagnostic is never even built and `exp` is never called.
  #[inline(always)]
  fn skip_and_report<M, Exp>(
    &mut self,
    skipped: Fetched<'inp, L>,
    frontier: &mut AtFrontier<L::Span, L::State>,
    exp: &mut Exp,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    M: ScanMode<'inp, L, Ctx, Lang, Cmpl>,
    Exp: FnMut() -> Option<Expected<'inp, <L::Token as Token<'inp>>::Kind>>,
  {
    let (spanned, state) = skipped.tok.into_components();
    let (span, tok) = spanned.into_components();

    // The skip settle observed: a skipped token is consumed (it settles behind the
    // frontier, never to be yielded again), so it flows to the committed-token side
    // channel exactly like a consume settle — and BEFORE the report's verdict, so a fatal
    // rejection propagates with the token's event and its commit paired (trip-commit).
    self.emitter().commit_token(&tok, &span);

    let report = M::REPORT_SKIPPED
      .then(|| UnexpectedToken::maybe_expected_of(span.clone(), exp()).with_found(tok));
    frontier.adopt(span, state);

    match report {
      Some(report) => self.emitter().emit_unexpected_token(report),
      None => Ok(()),
    }
  }

  /// Puts a token the scan decided **not** to consume back where an unconsumed token lives: the
  /// front of the stream. This is how every `to`-shaped stop settles — the recovery scans and the
  /// trivia skip alike — and it is the same call whichever origin the token came from, so the
  /// front after a stop holds that token either way and [`cursor`](Self::cursor) reads the same
  /// resume position either way.
  ///
  /// The put-back itself, and the push-history accounting the [`Origin`] exists for, live in
  /// [`hold_front`](Self::hold_front); this keeps the scanner-facing name.
  #[inline(always)]
  fn unconsume(&mut self, fetched: Fetched<'inp, L>) {
    let Fetched { tok, origin } = fetched;
    self.hold_front(tok, origin);
  }
}
