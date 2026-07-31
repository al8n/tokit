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
  /// The position a rewind resumes from, cloned at capture rather than read back at restore.
  ///
  /// `Emitter::rewind` needs the offset the parse returns to. That used to be obtained inside
  /// `restore_entry` by reading `cursor()` back off the input once the position had been
  /// reinstalled — an `L::Offset::clone`, caller code, standing between the restore and the
  /// rewind that completes it. A panic there skipped the rewind entirely and `ScanScope::drop`
  /// then RELEASED the mark instead, so an abandoned scan's diagnostics survived into a parse
  /// that had rewound past them.
  ///
  /// Hoisting that clone to the top of `restore_entry` was not enough, and the sweep in
  /// `r9_restore_entry_is_atomic_at_every_offset_clone` is what showed it: wherever inside the
  /// body the clone sits, failing it still skips the rewind. The operation has to leave the body.
  /// Captured here it runs BEFORE the emitter mark is taken, where the only thing an unwind
  /// discards is a half-built snapshot that owes nothing to anyone.
  rewind_to: Offset,
  /// The poison latch as it stood at entry.
  ///
  /// The fifth fact, and the one this snapshot was missing. A limit trip latches the boundary
  /// inside `classify` **before** its diagnostic is emitted, so an unwind out of the diagnostic
  /// path — or out of any caller code after the latch — reaches a rewinding mode's restore with a
  /// boundary the rewound lineage never produced. Restoring span, state, watermark and mark and
  /// leaving that standing poisons the input at a position nothing committed ever reached, with no
  /// diagnostic to show for it. `Checkpoint` has always carried it; this memo had not.
  poison_boundary: Option<Offset>,
}

impl<Span, State, Offset> ThroughEntry<Span, State, Offset> {
  /// The emitter mark this entry captured. Read by the committing modes' unwind edge, which
  /// keeps its position and therefore its emissions, so its entry mark settles by `release`
  /// rather than by the rewind an abandoning exit performs.
  #[inline(always)]
  pub(super) const fn mark(&self) -> u64 {
    self.mark
  }

  /// Hands over all five facts at once, so the one restore body destructures rather than
  /// reaching through five borrows.
  #[inline(always)]
  #[allow(clippy::type_complexity)]
  pub(super) fn into_components(self) -> (Span, State, u64, Offset, Offset, Option<Offset>) {
    (
      self.span,
      self.state,
      self.mark,
      self.error_end,
      self.rewind_to,
      self.poison_boundary,
    )
  }

  /// Bundles the five facts the end-of-input rewind restores.
  #[inline(always)]
  pub(super) const fn new(
    span: Span,
    state: State,
    mark: u64,
    error_end: Offset,
    rewind_to: Offset,
    poison_boundary: Option<Offset>,
  ) -> Self {
    Self {
      span,
      state,
      mark,
      error_end,
      rewind_to,
      poison_boundary,
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
  /// Settle the token the scan stopped on, and produce the carried token — **the stopping
  /// token only**. A `to`-shaped mode leaves it unconsumed at the front of the stream and
  /// returns `None`; a `through` mode consumes it (commits at its span, adopting the state that
  /// produced it) and returns `Some(tok)`.
  ///
  /// This used to also commit the frontier, and fusing the two was a window: both were handed
  /// over before the scope was disarmed, so a panic in the first — `unconsume` reaches the
  /// public [`Cache::push_front`](crate::cache::Cache::push_front), which nothing makes
  /// infallible — took the frontier down with it and the diagnosed prefix was never committed. A
  /// catching host that retried then re-lexed and re-diagnosed the whole prefix. One handover per
  /// call: the scope still owns the frontier while this runs, so its `Drop` can still commit it.
  fn settle_stop(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    stopper: Fetched<'inp, L>,
  ) -> Option<Spanned<L::Token, L::Span>>;

  /// Whether a stop then commits the scan's frontier — the skipped prefix's end.
  ///
  /// `true` for the `to`-shaped modes, which stop *before* the token and keep the prefix;
  /// `false` for [`SyncThrough`], which commits at the stopping token's own span instead and has
  /// no use for the frontier. A monomorphized constant, so each mode compiles to one arm.
  const COMMITS_FRONTIER_ON_STOP: bool;

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

  /// Settle the pre-call snapshot on an exit that **abandons** the scan: restore the state the
  /// snapshot names. The rewinding modes replay [`on_eof`](ScanMode::on_eof)'s body without the
  /// lexer (span, state, dedup watermark, then the emitter rewind to the mark); the committing
  /// modes hold no snapshot, so there is nothing to restore.
  ///
  /// This is the settle of the scanner's **seventh return exit** — a partial-input
  /// `Incomplete`, which commits nothing and so must leave nothing — and of the unwind edge for
  /// a rewinding mode. It is *not* reached on the committing modes' unwind edge, which keeps its
  /// diagnosed prefix instead; see [`ScanScope`]'s `Drop`.
  ///
  /// The committing modes hold no snapshot, so under [`Partial`](crate::input::Partial) the
  /// scanner captures a [`ThroughEntry`] of its own at entry and hands it in as `entry`; the
  /// rewinding modes' snapshot already *is* one, and their `entry` is `None` by construction.
  fn on_incomplete(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    snapshot: Self::Snapshot,
    entry: Option<ThroughEntry<L::Span, L::State, L::Offset>>,
  );

  /// The emitter mark this mode's snapshot holds, if it holds one. `None` for the committing
  /// modes, whose snapshot is `()`.
  ///
  /// Read once at scope construction so the scope can keep a `Copy` of the mark alongside the
  /// snapshot: an exit settle runs caller code, and if it unwinds after the snapshot has been
  /// handed over there is nothing else left that knows a mark is outstanding.
  fn mark_of(snapshot: &Self::Snapshot) -> Option<u64>;

  /// Whether this mode's [`Snapshot`](ScanMode::Snapshot) **is** a [`ThroughEntry`] — i.e.
  /// whether it already holds the five facts an abandoning exit restores. A monomorphized
  /// constant: each mode compiles to one arm of the split with the other eliminated.
  ///
  /// # This is a question about the MODE, and it is not a disposition
  ///
  /// It reads as "does this mode rewind?", and for three of the four modes that happens to be the
  /// same answer as "does this *exit* rewind?" — which is why it was used as a proxy for
  /// disposition and why the proxy held until [`SyncBalanced`] was instantiated in a stop-panic
  /// cell. `SyncBalanced` has `HOLDS_ENTRY = true` and
  /// [`COMMITS_FRONTIER_ON_STOP`](ScanMode::COMMITS_FRONTIER_ON_STOP)` = true`: it rewinds at end
  /// of input and keeps on a stop. Disposition belongs to the exit, so
  /// [`ScanScope`]'s `keep_on_unwind` carries it and this constant answers only the questions
  /// that are genuinely about the mode.
  ///
  /// The obligation that generalizes, recorded because it was paid for twice: **adding an
  /// axis that discriminates a case the old proxy conflated makes every existing reader of that
  /// proxy a suspect until it has been re-asked which question it wanted.** The three readers of
  /// this constant were swept when `COMMITS_FRONTIER_ON_STOP` was introduced:
  ///
  /// - the scope's `Drop` arm — wanted the **exit**'s disposition; now reads `keep_on_unwind`
  ///   first and this only for a mid-loop unwind, where no exit has decided anything;
  /// - `skip_until`'s Partial entry capture (`Cmpl::PARTIAL && !M::HOLDS_ENTRY`) — wants the
  ///   **mode**: whether the snapshot already carries what an abandoning exit restores. Correct
  ///   as written, and it must stay a mode question: the capture happens before any exit exists.
  /// - `SyncThrough::on_incomplete`'s `debug_assert!(entry.is_none())` — the same mode fact
  ///   restated as an assertion. Correct.
  const HOLDS_ENTRY: bool;
}

/// Where the token under decision is, and — when it is not here — **why**.
///
/// `Option<Fetched>` could not say that. `None` meant both "no token is out of the stream" and
/// "one was handed to a settle that may not have finished", and those need opposite repairs: the
/// first is already contiguous, the second is a hole between the committed position and whatever
/// the stream still retains. Every finding on this edge reduced to that conflation,
/// so the state names it.
enum TokenSlot<'inp, L>
where
  L: Lexer<'inp>,
{
  /// Nothing is out of the stream: either no token has been fetched this iteration, or the one
  /// that was is now recorded — put back at the front, or settled behind the frontier.
  Absent,
  /// The scope holds it. An unwind returns it to the front of the stream, and the stream is
  /// contiguous with the committed position again.
  Held(Fetched<'inp, L>),
  /// Handed to a settle that may not have completed. The token may be gone from the stream while
  /// the committed position is still *behind* it — so the retained stream is no longer adjacent to
  /// that position, and anything still resident is stale. An unwind must clear the stream: the
  /// token itself is not lost, because re-lexing from the committed position reproduces it and
  /// everything after it. What is lost is only the cache's memo of it.
  HandedOver,
}

impl<'inp, L> TokenSlot<'inp, L>
where
  L: Lexer<'inp>,
{
  /// The token, borrowed, for the one site that evaluates the predicate.
  #[inline(always)]
  fn held(&self) -> &Fetched<'inp, L> {
    match self {
      Self::Held(fetched) => fetched,
      _ => unreachable!("the predicate runs with the token held"),
    }
  }

  /// Takes the token for a settle that **records it as consumed** — the skip, whose very first
  /// act is `AtFrontier::adopt`, so the frontier covers it from that instant and the stream stays
  /// adjacent to the committed position.
  #[inline(always)]
  fn take_recorded(&mut self) -> Fetched<'inp, L> {
    match core::mem::replace(self, Self::Absent) {
      Self::Held(fetched) => fetched,
      _ => unreachable!("one take per fetch"),
    }
  }

  /// Takes the token for a settle that **may not complete** — the stop, whose put-back reaches the
  /// public `Cache::push_front` and whose `through` form reaches `Emitter::commit_token`. The slot
  /// remembers that, so an unwind knows the stream needs clearing rather than a put-back.
  #[inline(always)]
  fn hand_over(&mut self) -> Fetched<'inp, L> {
    match core::mem::replace(self, Self::HandedOver) {
      Self::Held(fetched) => fetched,
      _ => unreachable!("one take per fetch"),
    }
  }
}

/// The scan's owner for the duration of the loop — and the thing that makes an unwind an exit
/// like any other.
///
/// # The discipline, in one sentence
///
/// **Nothing that can unwind runs between taking something out of the stream and recording where
/// it went.** Four windows of this class have been found in this loop, three of them after the
/// scope existed, and each was the same shape rather than a new one:
///
/// - the **token** is owned here across the predicate, and `skip_and_report` performs its
///   frontier `adopt` as its *first* act, so the token is either wholly un-consumed (still in
///   `in_flight`, and the unwind edge puts it back) or wholly behind the frontier (and the unwind
///   edge commits at it). There is no third state, so the report's span clone, `commit_token` and
///   `exp` can all panic harmlessly;
/// - the **frontier** is filled *after* this value exists, because building it runs
///   `L::State: Clone` — caller code on the far side of the caller's capture;
/// - the **mark** is kept beside the snapshot (`outstanding_mark`), because an exit settle is
///   itself caller code and once the snapshot is handed over nothing else knows a mark is
///   outstanding.
///
/// Stated that way the discipline is checkable by reading straight down each body, and the fifth
/// window of the class is unreachable rather than merely unfound. What it does *not* cover is a
/// fact the snapshot never captured in the first place — see [`ThroughEntry`]'s poison-latch
/// field for the one member of that other kind.
///
/// `skip_until` pops each token out of durable state (the parked slot, or the cache front) and
/// then runs caller code over it: the predicate, the expected-tokens closure, the frontier's
/// `L::State: Clone`, the lexer itself. Every one of those can panic, and before this scope
/// existed a panic through any of them was an exit no put-back and no settle ever saw: the
/// in-flight token was dropped with an unowned local, the skipped prefix stayed behind a
/// frontier that was never committed, and a rewinding mode's entry mark was neither rewound nor
/// released. With a warm cache that lost tokens outright.
///
/// So the scope owns all three: the input, the frontier, and the token under decision. Its
/// `Drop` is armed by the presence of the snapshot and disarmed by every return exit taking it
/// out, so the six ordinary exits are behaviourally untouched and only an unwind reaches the
/// body below.
///
/// # The unwind edge is split by mode, and each half mirrors an exit the mode already owns
///
/// - **Committing modes** (`SyncTo`, `SkipWhile`) behave exactly as their **fatal exit** does:
///   commit the diagnosed prefix at the frontier and put the in-flight token back at the front
///   of the stream. Zero re-lex, zero budget re-burn, zero token loss — reports, `commit_token`
///   events and the position advance together, so a host that catches and retries resumes
///   *after* the diagnosed prefix with no duplicate reports.
/// - **Rewinding modes** (`SyncThrough`, `SyncBalanced`) behave exactly as their **no-match end
///   of input** does: restore the full pre-call state and rewind the mark. The stream is cleared
///   against the never-moved committed position, so the region re-lexes deterministically.
///
/// A uniform clear was tried and measured doing real harm on the committing path: it discarded
/// pre-entry cache entries, the re-lex re-burned a shared limit budget, the limiter tripped and
/// a poison boundary latched at a position the original lineage never reached — precisely the
/// harm the cache-rollback contract exists to prevent. The split is what that measurement bought.
///
/// The rewinding arm keeps a **priced residue**: at true end of input the cache is empty by
/// construction, so `on_eof`'s restore never faces a warm untouched suffix — the panic edge
/// does, and re-lexes it. The price is pinned by
/// `sync_through_warm_unwind_prices_its_re_lex`.
pub(super) struct ScanScope<'g, 'inp, 'closure, L, Ctx, Lang, Cmpl, M>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
  M: ScanMode<'inp, L, Ctx, Lang, Cmpl>,
{
  ir: &'g mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  /// The mode's pre-call snapshot. `Some` while the scan is undecided: it doubles as the
  /// arm/disarm flag, so a return exit disarms by the same `take` that settles.
  snapshot: Option<M::Snapshot>,
  /// The scan's uncommitted position, OWNED here so the unwind edge can commit it.
  ///
  /// `Option` rather than `mem::take`: `AtFrontier` has no usable `Default` — `L::State` is
  /// arbitrary user extras — so `mem::take` does not compile. It is also `None` for the window
  /// between the scope's construction and the frontier's own `L::State: Clone`, which is a
  /// fallible step on the far side of the caller's capture and therefore has to be inside.
  frontier: Option<AtFrontier<L::Span, L::State>>,
  /// The token under decision — owned here while caller code runs over it, and, once it leaves,
  /// still *accounted for* here. See [`TokenSlot`].
  token: TokenSlot<'inp, L>,
  /// The outstanding emitter mark, kept **beside** the snapshot rather than only inside it.
  ///
  /// An exit settle is caller code: `on_eof`/`on_incomplete` clone an offset for the emitter's
  /// cursor, `set_span` can construct a span, and `release`/`rewind` are foreign. Once the
  /// snapshot has been handed to the settle, nothing else in the program knows a mark is
  /// outstanding — so a panic mid-settle left it stranded with the scan's emissions standing.
  /// A `u64` is `Copy`, so keeping one here duplicates no ownership: the exit clears it once its
  /// settle has *finished*, and a `Drop` that finds it still set releases it. Release rather than
  /// rewind, deliberately: a rewind needs a cursor the half-done restore may no longer describe,
  /// while release is total, says exactly what is true (nothing will ever rewind to this mark),
  /// and reclaims the row. The emission state a half-run restore left behind is the panic's
  /// residue, bounded and documented — not a leak.
  outstanding_mark: Option<u64>,
  /// Whether the predicate has answered *stop* — the one fact about the exit that the scope's
  /// other fields cannot already tell it.
  ///
  /// This is deliberately **not** a disposition. A disposition is what the unwind edge should do,
  /// and that changes *during* an exit as the exit hands things over; a sticky verdict set once
  /// for a whole exit cannot express it, which is what a stored flag gets wrong. The
  /// disposition is derived instead — see [`ScanScope::keeps_on_unwind`].
  stop_decided: bool,
  /// A committing mode's own entry capture, taken only under [`Partial`](crate::input::Partial)
  /// — the four facts its `Incomplete` exit restores, which its `Snapshot` (`()`) does not hold.
  /// `None` under `Complete` (the capture is behind the mode/completeness consts and never
  /// monomorphizes) and `None` for the rewinding modes, whose snapshot already is one.
  entry: Option<ThroughEntry<L::Span, L::State, L::Offset>>,
}

impl<'inp, L, Ctx, Lang, Cmpl, M> ScanScope<'_, 'inp, '_, L, Ctx, Lang, Cmpl, M>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
  M: ScanMode<'inp, L, Ctx, Lang, Cmpl>,
{
  /// Whether an unwind from **right here** should keep the scan's progress or abandon it.
  ///
  /// Derived, not stored, because the answer changes as an exit hands things over — which is the
  /// defect a stored sticky flag leaves in place. Two clauses:
  ///
  /// - a **committing mode** always keeps: that is the ratified posture, measured against the
  ///   alternative;
  /// - a **rewinding mode** keeps only once the predicate has answered *stop* **and** the scope can
  ///   still perform the commit that a stop's disposition requires. `frontier.is_some()` is that
  ///   second condition, and it is why this cannot be a flag: the frontier leaves mid-exit.
  ///
  /// Both halves matter, and each was a finding. Without the first clause `SyncBalanced`'s
  /// interrupted stop threw away a prefix its own stop keeps. Without the second, an interrupted
  /// `commit_at` left a rewinding mode's emissions describing a prefix the position does not
  /// cover — duplicate diagnostics on retry, where abandoning rewinds them.
  ///
  /// `M::HOLDS_ENTRY` is a constant, so a committing mode folds this to `true` and drops the
  /// abandon arm entirely.
  #[inline(always)]
  fn keeps_on_unwind(&self) -> bool {
    !M::HOLDS_ENTRY || (self.stop_decided && self.frontier.is_some())
  }

  /// The frontier, borrowed. Live for the whole loop: only a return exit takes it out, and
  /// every return exit that does so leaves immediately.
  #[inline(always)]
  fn live_frontier(&self) -> &AtFrontier<L::Span, L::State> {
    self
      .frontier
      .as_ref()
      .expect("the frontier is live for the whole loop")
  }

  /// Takes the frontier for a return exit that commits at it.
  ///
  /// # Why every caller is `take_frontier()` immediately followed by `commit_at(..)`
  ///
  /// That adjacency is the discipline, not a coincidence, and it is what the three
  /// commit-at-the-frontier exits (the boundary drain, the trip, and the fatal skip report) rely
  /// on: the *only* code that runs while the frontier is out of the scope is the call that
  /// records it. `commit_at` can still panic — it moves the span through `set_span`, whose clamp
  /// branch constructs an `L::Span` — but no ordering rescues a value being handed to the call
  /// that consumes it, so there is nothing left to defend there and nothing to change.
  ///
  /// What is *not* safe is putting fallible work between the take and the record, which is what
  /// the stop exit used to do by fusing the token settle and the frontier commit into one hook.
  /// If a caller ever needs work in between, the frontier has to stay in the scope for it.
  #[inline(always)]
  fn take_frontier(&mut self) -> AtFrontier<L::Span, L::State> {
    self
      .frontier
      .take()
      .expect("the frontier is live for the whole loop")
  }

  /// Takes the snapshot out — which both hands it to the exit's own settle and **disarms** the
  /// unwind edge. Every return exit calls this exactly once; the census locks the pairing.
  #[inline(always)]
  fn disarm(&mut self) -> M::Snapshot {
    self.snapshot.take().expect("one settle per exit")
  }

  /// Records that an exit's settle has **finished**, so the mark it spent is no longer this
  /// scope's problem. Called after the settle returns, never before: the gap between `disarm` and
  /// this call is exactly the window the fallback in `Drop` covers.
  #[inline(always)]
  fn settled(&mut self) {
    self.outstanding_mark = None;
  }

  /// Settles the Partial entry on an exit that KEEPS the scan's progress: the position stands,
  /// so the emissions made over it stand, so the mark is released rather than rewound — the
  /// same keep-dual [`ScanMode::on_commit`] performs for a rewinding mode's own snapshot. A
  /// no-op under `Complete` and for the rewinding modes, where there is no entry to settle.
  #[inline(always)]
  fn keep_entry(&mut self) {
    if let Some(entry) = self.entry.take() {
      self.ir.emitter().release(entry.mark());
    }
    self.outstanding_mark = None;
  }

  /// Takes both the snapshot and the entry for the abandoning exit, which settles them through
  /// [`ScanMode::on_incomplete`] — and disarms the scope in the same move.
  #[inline(always)]
  fn abandon(
    &mut self,
  ) -> (
    M::Snapshot,
    Option<ThroughEntry<L::Span, L::State, L::Offset>>,
  ) {
    (self.disarm(), self.entry.take())
  }

  /// Runs the skip-and-report through the owned frontier.
  #[inline(always)]
  fn skip_one<Exp>(
    &mut self,
    skipped: Fetched<'inp, L>,
    exp: &mut Exp,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Exp: FnMut() -> Option<Expected<'inp, <L::Token as Token<'inp>>::Kind>>,
  {
    let frontier = self
      .frontier
      .as_mut()
      .expect("the frontier is live for the whole loop");
    self.ir.skip_and_report::<M, _>(skipped, frontier, exp)
  }
}

impl<'inp, L, Ctx, Lang, Cmpl, M> Drop for ScanScope<'_, 'inp, '_, L, Ctx, Lang, Cmpl, M>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
  M: ScanMode<'inp, L, Ctx, Lang, Cmpl>,
{
  fn drop(&mut self) {
    let keep = self.keeps_on_unwind();
    let Some(snap) = self.snapshot.take() else {
      // Disarmed: a return exit took the snapshot. If it also finished its settle, there is
      // nothing left; if it unwound part-way through, the mark it was spending is still ours.
      if let Some(mark) = self.outstanding_mark.take() {
        self.ir.emitter().release(mark);
      }
      return;
    };
    // The arm below settles the mark itself, through the mode's own hook or through the entry
    // keep. Clear the fallback first: a panic inside a settle running from a `Drop` mid-unwind
    // aborts either way, so there is no third state to defend.
    self.outstanding_mark = None;
    let token = core::mem::replace(&mut self.token, TokenSlot::Absent);
    if keep {
      // ── keep: `SyncTo::on_stop`, byte for byte, plus whatever the token's state requires ──
      match token {
        // The scope still had it, so the put-back restores adjacency and nothing else is stale.
        // NOT a clear: clearing here was measured discarding pre-entry entries, re-burning a
        // shared limit budget and tripping a limiter the honest run never trips.
        TokenSlot::Held(fetched) => self.ir.unconsume(fetched),
        // A settle took it and may not have finished, so the committed position is behind a token
        // the stream no longer holds and everything still resident sits on the far side of that
        // hole. Clear, and the region re-lexes from the committed position — which reproduces the
        // token itself, so nothing is lost but the memo. This costs a re-lex of the resident
        // suffix on a path that is already panicking, which is the same trade the rewinding arm
        // prices, and it is the alternative to skipping a token in silence.
        TokenSlot::HandedOver => self.ir.unwind_clear_stream(),
        TokenSlot::Absent => {}
      }
      if let Some(frontier) = self.frontier.take() {
        self.ir.commit_at(frontier);
      }
      // The position is KEPT, so the emissions made over it are kept too. Both marks settle as
      // kept: the mode's own snapshot through `on_commit` (a no-op for the committing modes,
      // whose snapshot is `()`, and the release a rewinding mode's entry mark is owed when this
      // arm runs for it), and the Partial entry through the same keep body the five keeping
      // return exits use.
      M::on_commit(self.ir, snap);
      self.keep_entry();
    } else {
      // ── abandon: the rewinding modes' own no-match end-of-input settle ──
      // The token's region re-lexes with the cleared store whatever state it was in, so dropping
      // it loses nothing: the restore puts the committed position back behind it.
      drop(token);
      self.ir.unwind_clear_stream();
      M::on_incomplete(self.ir, snap, self.entry.take());
    }
  }
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
  fn settle_stop(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    stopper: Fetched<'inp, L>,
  ) -> Option<Spanned<L::Token, L::Span>> {
    // Leave the token unconsumed — which in this crate means AT THE FRONT OF THE STREAM, the home
    // of every lexed-but-not-consumed token (`try_expect`'s decline puts one back there too) and
    // the one place `cursor()` reads. The caller peeks it straight back out. Doing this for a token
    // the loop LEXED, and not only for one it popped, is what makes the resume cursor after a stop
    // a fact about the stream instead of about the caller's lookahead depth.
    //
    // The frontier commit that pairs with this is the caller's, and it happens after: this call
    // reaches public cache code, and the scope has to still own the frontier while it does.
    ir.unconsume(stopper);
    None
  }

  const COMMITS_FRONTIER_ON_STOP: bool = true;

  #[inline(always)]
  fn on_eof(ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, lexer: L, _snapshot: ()) {
    // Nothing stopped the scan: commit the whole skipped run at the lexer's end. `sync_to` reports
    // as it goes and keeps that progress, so end of input is not a rewinding failure here.
    // BOTH operands are caller code, and both are evaluated before either half of the position is
    // written. Reading the span, writing it, and only then running `into_state` left the span at
    // the lexer's end paired with the entry state when `into_state` unwound.
    let span = lexer.span();
    let state = lexer.into_state();
    ir.commit_position(span.into(), state);
  }

  #[inline(always)]
  fn on_commit(_ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, _snapshot: ()) {
    // A committing mode snapshots nothing, so a kept exit has nothing to release.
  }

  #[inline(always)]
  fn mark_of(_snapshot: &()) -> Option<u64> {
    None
  }

  #[inline(always)]
  fn on_incomplete(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    _snapshot: (),
    entry: Option<ThroughEntry<L::Span, L::State, L::Offset>>,
  ) {
    // A committing mode snapshots nothing, so the four facts come from the scanner's own
    // Partial-only entry capture — and under `Complete` there is none, which is what erases
    // this whole body from the complete path.
    if let Some(entry) = entry {
      <SyncThrough as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::on_incomplete(ir, entry, None);
    }
  }

  const HOLDS_ENTRY: bool = false;
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
  fn settle_stop(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    stopper: Fetched<'inp, L>,
  ) -> Option<Spanned<L::Token, L::Span>> {
    // Consume the match: commit at its span, adopting the state that produced it. This is
    // `consume_cached_one`'s body over the same carrier — and it is the same two lines whether the
    // token was popped off the cache or lexed a moment ago, because a `CachedToken` carries the
    // post-token state either way.
    let (tok, state) = stopper.tok.into_components();
    ir.commit_token(tok.data(), tok.span_ref(), state);
    Some(tok)
  }

  /// A `through` stop commits at the stopping token's own span, so the frontier — the skipped
  /// prefix's end — is behind it and simply dropped.
  const COMMITS_FRONTIER_ON_STOP: bool = false;

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
    ir.restore_entry(snapshot);
  }

  #[inline(always)]
  fn on_commit(ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>, snapshot: Self::Snapshot) {
    // The scan kept its progress, so the entry snapshot will never be restored: release the
    // emitter mark it captured — the keep-dual of `on_eof`'s rewind. Advisory (a no-op for
    // every built-in emitter); a mark-keyed emitter reclaims its row here instead of
    // stranding one per successful sync.
    ir.emitter().release(snapshot.mark);
  }

  #[inline(always)]
  fn mark_of(snapshot: &ThroughEntry<L::Span, L::State, L::Offset>) -> Option<u64> {
    Some(snapshot.mark())
  }

  #[inline(always)]
  fn on_incomplete(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    snapshot: ThroughEntry<L::Span, L::State, L::Offset>,
    entry: Option<ThroughEntry<L::Span, L::State, L::Offset>>,
  ) {
    debug_assert!(
      entry.is_none(),
      "a rewinding mode's snapshot IS the entry; a second capture would be a second mark",
    );
    // `on_eof`'s body without the lexer argument: the exit that reaches this abandoned nothing
    // to the lexer, so the restore is the same five facts and the same single mark settle.
    ir.restore_entry(snapshot);
  }

  const HOLDS_ENTRY: bool = true;
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
  fn settle_stop(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    stopper: Fetched<'inp, L>,
  ) -> Option<Spanned<L::Token, L::Span>> {
    // Stop before the sync point, exactly as `sync_to` does — which is also what places the
    // zero-skip hole: `sync_balanced` anchors it at `cursor()`, and the cursor is the match's start
    // because the match is left at the front of the stream here, cached, parked or lexed.
    <SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::settle_stop(ir, stopper)
  }

  const COMMITS_FRONTIER_ON_STOP: bool =
    <SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::COMMITS_FRONTIER_ON_STOP;

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

  #[inline(always)]
  fn mark_of(snapshot: &Self::Snapshot) -> Option<u64> {
    <SyncThrough as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::mark_of(snapshot)
  }

  #[inline(always)]
  fn on_incomplete(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    snapshot: Self::Snapshot,
    entry: Option<ThroughEntry<L::Span, L::State, L::Offset>>,
  ) {
    <SyncThrough as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::on_incomplete(ir, snapshot, entry)
  }

  const HOLDS_ENTRY: bool = true;
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
  fn settle_stop(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    stopper: Fetched<'inp, L>,
  ) -> Option<Spanned<L::Token, L::Span>> {
    <SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::settle_stop(ir, stopper)
  }

  const COMMITS_FRONTIER_ON_STOP: bool =
    <SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::COMMITS_FRONTIER_ON_STOP;

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

  #[inline(always)]
  fn mark_of(snapshot: &()) -> Option<u64> {
    <SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::mark_of(snapshot)
  }

  #[inline(always)]
  fn on_incomplete(
    ir: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    snapshot: (),
    entry: Option<ThroughEntry<L::Span, L::State, L::Offset>>,
  ) {
    <SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::on_incomplete(ir, snapshot, entry)
  }

  const HOLDS_ENTRY: bool = false;
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
  /// # Every exit settles the pre-call snapshot exactly once — the unwind edge included
  ///
  /// The loop has six *return* exits. Five keep the scan's progress — the boundary drain, a
  /// fatal lexer-error rejection propagating out of [`scan_with`](Self::scan_with), a limit
  /// trip, a stop, and a fatal skipped-token report — and each settles the mode's snapshot
  /// through [`ScanMode::on_commit`] after its own position settle (a rewinding mode releases
  /// the emitter mark it captured at entry; the committing modes hold none). The sixth is end
  /// of input, where [`ScanMode::on_eof`] spends the snapshot instead — for the rewinding
  /// modes, by rewinding to its mark.
  ///
  /// A **seventh** exit is not a return at all: a panic through one of the six caller-code
  /// windows this loop runs (the predicate, `exp`, the frontier's `L::State: Clone`, the
  /// lexer's own code, `commit_token`, and the emitter's report). [`ScanScope`] owns the input,
  /// the frontier and the in-flight token for the whole loop precisely so that edge settles
  /// too: per mode, either the committing keep (`unconsume` + commit at the frontier) or the
  /// rewinding restore (clear the stream, then [`ScanMode::on_incomplete`]). Each return exit
  /// takes the snapshot out, which is also what disarms the scope — so one snapshot, one
  /// settle, per call, on every exit (RELEASE_CENSUS).
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
    // The lexer, built the moment the cache runs out — under the frontier's state and at its end,
    // which is exactly where the drained cache left the lex position. A call answered entirely out
    // of the cache never builds one.
    let mut lexing: Option<Resume<L, L::Offset>> = None;

    // The scope takes ownership of the input, the snapshot, the frontier and the token under
    // decision, so that an unwind through any caller-code window is an exit with a settle. It is
    // constructed with NO frontier and then filled: the frontier's `L::State: Clone` is caller
    // code, and it runs on the far side of the caller's capture — build it outside and a panic
    // there drops the snapshot raw, with the mark already taken and nobody left to settle it.
    // A MODE question, deliberately, and swept as one when `COMMITS_FRONTIER_ON_STOP` was
    // introduced: this asks whether the mode's own snapshot already carries what an abandoning
    // exit restores, which is settled before any exit exists and cannot be a disposition.
    //
    // Under `Partial`, a committing mode's `Incomplete` exit has to restore five facts its
    // `Snapshot` (`()`) does not hold, so the scanner captures them itself — and only here:
    // `Cmpl::PARTIAL` is `false` under `Complete` and `M::HOLDS_ENTRY` is `true` for the modes
    // whose snapshot already is an entry, so this whole expression is eliminated at
    // monomorphization on every path but the one that needs it. CAPTURE_WINDOW: the fallible
    // `L::Offset::clone` is evaluated first, exactly as the three caller-side captures do, and
    // the mark is born directly into the scope that owns it.
    let entry = if Cmpl::PARTIAL && !M::HOLDS_ENTRY {
      let error_end = self.emitted_error_end.clone();
      let rewind_to = self.span.end_ref().clone();
      let span = self.span.clone();
      let state = self.state.clone();
      let latch = self.latch_snapshot();
      Some(ThroughEntry::new(
        span,
        state,
        self.session.emitter.checkpoint(),
        error_end,
        rewind_to,
        latch,
      ))
    } else {
      None
    };

    // The one outstanding mark, read out before the snapshot is handed to the scope. At most one
    // of the two is ever `Some`: a mode either holds its own entry (`HOLDS_ENTRY`) or takes the
    // Partial one above, never both.
    let outstanding_mark = M::mark_of(&snapshot).or_else(|| entry.as_ref().map(ThroughEntry::mark));

    let mut scope: ScanScope<'_, 'inp, '_, L, Ctx, Lang, Cmpl, M> = ScanScope {
      ir: self,
      snapshot: Some(snapshot),
      frontier: None,
      token: TokenSlot::Absent,
      entry,
      outstanding_mark,
      // Mid-loop until the predicate says otherwise, so the mode's own posture is the answer.
      stop_decided: false,
    };
    // The scan's uncommitted position: the pre-call span/state, then the end of each token the loop
    // settles behind it. A trip latches here, and every stop that keeps the loop's progress commits
    // here.
    scope.frontier = Some(AtFrontier {
      span: scope.ir.span.clone(),
      state: scope.ir.state.clone(),
    });

    loop {
      // ── The one place the two origins differ: where the next token comes from ──
      // A retained token arrives with the state that lexed it. One popped off the CACHE was
      // already counted by the peek that cached it; one taken out of the parked slot never was,
      // so putting it back is a genuinely new entry for a cache that now has room.
      let fetched = if Self::can_park() && scope.ir.has_front_parked() {
        Fetched {
          tok: scope.ir.take_front_parked().expect("probed just above"),
          origin: Origin::Parked,
        }
      } else if let Some(tok) = scope.ir.take_front() {
        // Reached with the parked slot provably empty, so this is the cache pop.
        Fetched {
          tok,
          origin: Origin::Cache,
        }
      } else {
        if lexing.is_none() {
          let at = scope.live_frontier().span.end_ref().clone();
          // A sticky limit trip latches a poison boundary: once the lex position has reached the
          // durable frontier there is no token left to scan. Commit what the loop already
          // skipped — real progress — and yield the exhausted outcome without rebuilding a lexer.
          if scope.ir.reached_boundary(&at) {
            // Take-then-record, adjacent: see `take_frontier`.
            let frontier = scope.take_frontier();
            scope.ir.commit_at(frontier);
            let snapshot = scope.disarm();
            M::on_commit(scope.ir, snapshot);
            scope.settled();
            scope.keep_entry();
            return Ok(Scanned::Exhausted);
          }
          lexing = Some(
            scope
              .ir
              .resume_at_frontier(scope.frontier.as_ref().expect("live")),
          );
        }
        let resume = lexing.as_mut().expect("the lexer is built just above");
        // `scan_with` centralizes the poison latch, the dedup watermark, the partial-input
        // frontier rules, and the fatal-emit discipline, handing back only the events this loop
        // must decide.
        let scanned = scope
          .ir
          .scan_with(resume.parts_mut(), scope.frontier.as_ref().expect("live"));
        match scanned {
          Ok(Scan::Token(tok)) => Fetched {
            tok: CachedToken::new(tok, resume.lexer().state().clone()),
            origin: Origin::Lexer,
          },
          Ok(Scan::Tripped) => {
            // Commit the skipped prefix at the durable frontier — the end of the last skipped
            // token — so a later scan yields the poisoned outcome there instead of stranding
            // those tokens at the cursor. That commit is real progress, so any diagnostics made
            // over it persist.
            // Take-then-record, adjacent: see `take_frontier`.
            let frontier = scope.take_frontier();
            scope.ir.commit_at(frontier);
            let snapshot = scope.disarm();
            M::on_commit(scope.ir, snapshot);
            scope.settled();
            scope.keep_entry();
            return Ok(Scanned::Exhausted);
          }
          Ok(Scan::Eof) => {
            let lexer = lexing
              .take()
              .expect("the lexer is built just above")
              .into_lexer();
            let snapshot = scope.disarm();
            M::on_eof(scope.ir, lexer, snapshot);
            scope.settled();
            scope.keep_entry();
            return Ok(Scanned::Exhausted);
          }
          Err(e) => {
            // A fatal rejection of a crossed lexer error's diagnostic: `settle_fatal` already
            // committed the position inside `scan_with`, so this exit KEEPS the scan's progress
            // and the entry snapshot will never be restored — settle it as kept. The frontier
            // is deliberately not committed here, exactly as before: the rejected item is an
            // error, not a skipped token.
            if Cmpl::PARTIAL && Cmpl::is_incomplete_error(&e) {
              // The SEVENTH return exit. `settle_fatal` did not run — an `Incomplete` commits
              // nothing — so the fatal arm's "the position is already committed" reasoning does
              // not hold here, and everything the scan already did (each skipped token's
              // `commit_token`, each skip report, the dedup watermark it lifted) has to come
              // back off. Restore to entry: an incomplete attempt leaves no trace, which is what
              // makes refill-and-retry idempotent.
              let (snapshot, entry) = scope.abandon();
              M::on_incomplete(scope.ir, snapshot, entry);
              scope.settled();
              return Err(e);
            }
            let snapshot = scope.disarm();
            M::on_commit(scope.ir, snapshot);
            scope.settled();
            scope.keep_entry();
            return Err(e);
          }
        }
      };

      // ── One decision, one report, one settle — all of it blind to the origin ──
      // `pred` sees each token EXACTLY once, at this single site — and it sees it while the
      // SCOPE holds it, so a panicking predicate cannot take it out of the stream.
      scope.token = TokenSlot::Held(fetched);
      let stops = pred(scope.token.held().tok.token());

      if stops {
        // The predicate has answered. `stop_decided` is the fact; the disposition is derived from
        // it and from what the scope still holds, so it stays correct as the handovers proceed.
        scope.stop_decided = true;
        // ONE handover per call, each straight into its destination, and each announced: the
        // token goes through `hand_over`, so an unwind out of the settle knows the stream lost a
        // token the committed position is still behind and must be cleared rather than patched.
        // The frontier is still the scope's while that runs, and moves only afterwards — take
        // immediately followed by the commit that records it.
        let carried = M::settle_stop(scope.ir, scope.token.hand_over());
        if M::COMMITS_FRONTIER_ON_STOP {
          let frontier = scope.take_frontier();
          scope.ir.commit_at(frontier);
        } else {
          // A `through` stop committed at the token's own span; the prefix frontier is behind it.
          scope.frontier = None;
        }
        let snapshot = scope.disarm();
        M::on_commit(scope.ir, snapshot);
        scope.settled();
        scope.keep_entry();
        return Ok(Scanned::Found(carried));
      }

      // `take_recorded`, not `hand_over`: `skip_and_report`'s first act is the frontier `adopt`,
      // with only moves ahead of it, so from the instant the skip begins the frontier accounts for
      // the token and the stream stays adjacent to the committed position.
      let fetched = scope.token.take_recorded();
      if let Err(e) = scope.skip_one::<Exp>(fetched, &mut exp) {
        // The family's fatal-exit discipline: the token that trips a fatal emitter is committed,
        // and the error propagates. The commit lands at the frontier — the skipped token's end,
        // with the state that produced it — because `skip_and_report` settled it there before
        // honouring the verdict. It also carries the prefix this loop already diagnosed, so nothing
        // already reported is left to be reported again. `skip_one` already consumed the token,
        // so only the frontier is in play here — take-then-record, adjacent: see `take_frontier`.
        let frontier = scope.take_frontier();
        scope.ir.commit_at(frontier);
        let snapshot = scope.disarm();
        M::on_commit(scope.ir, snapshot);
        scope.settled();
        scope.keep_entry();
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

    // THE ADOPT IS THIS BODY'S FIRST ACT, and that ordering is the whole of its unwind safety.
    //
    // The token has already been taken out of the stream and out of the scope's `in_flight` — a
    // skipped token is consumed — so between here and the frontier recording it there must be
    // NOTHING that can unwind. Three caller-code steps follow, and all three are below the line:
    // the report's span clone (`L::Span::clone`), `commit_token` (a foreign emitter), and `exp`
    // (the caller's own closure). A panic in any of them now finds the token already behind the
    // frontier, which is exactly what the committing unwind edge commits at.
    //
    // The report's span therefore has to be read back OFF the frontier, the same way
    // `commit_token`'s is — one clone, still inside the mode const, so the trivia path and the
    // balanced scan pay nothing. The clone above the adopt was the last window of this class:
    // in a warm-cache `sync_to` it left the token in neither the scope nor the frontier, and the
    // drop committed the previous frontier with a consumed token accounted for nowhere.
    //
    // Adopting stays a SINGLE call site: the census locks the skip settle to one.
    let replaced = frontier.adopt(span, state);

    // The skip settle observed: a skipped token flows to the committed-token side channel exactly
    // like a consume settle — and BEFORE the report's verdict, so a fatal rejection propagates
    // with the token's event and its commit paired (trip-commit).
    self.emitter().commit_token(&tok, &frontier.span);
    // Only now: the adopted pair is recorded and the observer has been told, so a `Drop` that
    // unwinds here leaves both facts in place rather than a frontier the observer never saw.
    drop(replaced);

    let report = M::REPORT_SKIPPED
      .then(|| UnexpectedToken::maybe_expected_of(frontier.span.clone(), exp()).with_found(tok));

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
