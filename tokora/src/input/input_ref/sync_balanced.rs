use derive_more::IsVariant;

use crate::span::SimpleSpan;

use super::scan::{Scanned, SyncBalanced, ThroughEntry};
use super::*;

/// How a token kind participates in delimiter nesting: it opens a pair, closes a pair, or is
/// neutral.
///
/// This is the classification [`DelimClass`] produces and
/// [`sync_balanced`](InputRef::sync_balanced) consumes for its depth counting. `P` is the
/// caller's pair identity type (`'('`-vs-`'['` style), compared with `PartialEq` where a
/// consumer needs to pair an opener with its closer; the balanced scan itself counts depth
/// without enforcing pair correspondence (see the contract on
/// [`sync_balanced`](InputRef::sync_balanced)). Downstream dialects supply the pair tables
/// from their own token kinds; tokora ships only the vocabulary.
///
/// ```
/// use tokora::input::Balance;
///
/// let open = Balance::Open('(');
/// let close = Balance::Close('(');
/// assert!(open.is_open() && close.is_close());
/// assert_eq!(open.pair(), close.pair());
/// assert_eq!(Balance::<char>::Neutral.pair(), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IsVariant)]
pub enum Balance<P> {
  /// The kind opens a `P` pair (`(`, `[`, `{`, …): nesting depth increases past it.
  Open(P),
  /// The kind closes a `P` pair (`)`, `]`, `}`, …): nesting depth decreases past it
  /// (saturating at zero — a stray closer never drives the depth negative).
  Close(P),
  /// The kind takes no part in nesting.
  Neutral,
}

impl<P> Balance<P> {
  /// Returns the pair identity this kind opens or closes, or `None` for a neutral kind.
  #[inline(always)]
  pub const fn pair(&self) -> Option<&P> {
    match self {
      Self::Open(p) | Self::Close(p) => Some(p),
      Self::Neutral => None,
    }
  }
}

/// Classifies token kinds as delimiters for balanced synchronization: the caller-supplied side
/// of [`sync_balanced`](InputRef::sync_balanced)'s nesting awareness.
///
/// Implemented automatically for closures of shape `FnMut(&Kind) -> Balance<P>`, so a match
/// over the dialect's kind enum is a classifier:
///
/// ```
/// use tokora::input::{Balance, DelimClass};
///
/// #[derive(Clone, Copy)]
/// enum Kind {
///   LParen,
///   RParen,
///   Ident,
/// }
///
/// let mut classify = |kind: &Kind| match kind {
///   Kind::LParen => Balance::Open('('),
///   Kind::RParen => Balance::Close('('),
///   Kind::Ident => Balance::Neutral,
/// };
/// assert!(DelimClass::classify(&mut classify, &Kind::LParen).is_open());
/// assert!(DelimClass::classify(&mut classify, &Kind::Ident).is_neutral());
/// ```
pub trait DelimClass<Kind> {
  /// The pair identity type carried by [`Balance::Open`]/[`Balance::Close`].
  type Pair;

  /// Classifies `kind` as opening a pair, closing a pair, or neutral.
  fn classify(&mut self, kind: &Kind) -> Balance<Self::Pair>;
}

impl<F, Kind, P> DelimClass<Kind> for F
where
  F: FnMut(&Kind) -> Balance<P>,
{
  type Pair = P;

  #[inline(always)]
  fn classify(&mut self, kind: &Kind) -> Balance<P> {
    (self)(kind)
  }
}

/// The region a successful [`sync_balanced`](InputRef::sync_balanced) skipped: its source
/// span and how many tokens it dropped.
///
/// This is tokora's entire output for a balanced skip — consumers decide what it means (an
/// error node's extent, a "skipped N tokens" note, nothing). The same two facts reach the
/// emitter once per hole through
/// [`Emitter::emit_skipped_region`](crate::Emitter::emit_skipped_region).
///
/// A zero-skip hole (`skipped == 0`, a zero-width span at the resume position) records that
/// the sync point was already at hand; no skipped-region diagnostic is emitted for it.
///
/// ```
/// use tokora::{SimpleSpan, input::Hole};
///
/// let hole = Hole::new(SimpleSpan::new(2, 7), 3);
/// assert_eq!(hole.span(), SimpleSpan::new(2, 7));
/// assert_eq!(hole.skipped(), 3);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hole<S = SimpleSpan> {
  span: S,
  skipped: usize,
}

impl<S> Hole<S> {
  /// Bundles the span of a skipped region with its token count.
  #[inline(always)]
  pub const fn new(span: S, skipped: usize) -> Self {
    Self { span, skipped }
  }

  /// Returns the span covering the skipped region.
  #[inline(always)]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns a reference to the span covering the skipped region.
  #[inline(always)]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns the number of tokens the skip dropped.
  #[inline(always)]
  pub const fn skipped(&self) -> usize {
    self.skipped
  }

  /// Consumes the hole and returns the span of the skipped region.
  #[inline(always)]
  pub fn into_span(self) -> S {
    self.span
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  /// Skip tokens, nesting-aware, until `pred` matches at delimiter depth zero; stops *before*
  /// the matching token and returns the [`Hole`] describing the skipped region.
  ///
  /// `classifier` names which token kinds open and close pairs ([`DelimClass`] /
  /// [`Balance`]); `pred` is the depth-0 sync predicate. Each scanned token is decided in
  /// this order:
  ///
  /// - at depth zero, `pred` is consulted **first** — so an opener or a stray closer that is
  ///   itself a sync point (the classic `}` recovery target) syncs rather than counting;
  /// - otherwise the token is skipped into the hole: an [`Balance::Open`] kind increments the
  ///   depth, a [`Balance::Close`] kind decrements it **saturating at zero** (a stray closer
  ///   at depth zero is plain garbage — skipped, never driving the depth negative), and a
  ///   [`Balance::Neutral`] kind leaves it unchanged. Strictly above depth zero `pred` is
  ///   never consulted, so garbage containing balanced pairs skips over enclosed sync-set
  ///   tokens.
  ///
  /// Depth counting is **token-level**, which leans on a lexer-contract clause: a composite
  /// token (a block string, a raw literal) is one token whose lexer already swallowed any
  /// delimiter characters inside it, so nothing inside a token can affect the depth. The
  /// count is also **pair-blind**: a closer closes the innermost open pair regardless of its
  /// [`Balance::pair`] identity — inside garbage, mismatched pairs are part of what is being
  /// skipped, and the parse that resumes at the sync point decides what they meant.
  ///
  /// # One diagnostic per hole
  ///
  /// The skipped tokens are **not** reported individually. A successful sync that skipped at
  /// least one token reports the whole region exactly once through
  /// [`Emitter::emit_skipped_region`](crate::Emitter::emit_skipped_region), with the hole's
  /// span and count; a zero-skip success (the sync point was the very next token) emits
  /// nothing. Genuine lexer errors crossed while skipping are still emitted (deduplicated)
  /// along the way, and they are not counted into `skipped` — the count covers valid tokens
  /// only. A fatal emitter rejection mid-skip follows the sync family's fatal-exit
  /// discipline: the error token is committed and the error propagates, exactly as in
  /// [`sync_through`](Self::sync_through).
  ///
  /// # Diagnostics travel with progress
  ///
  /// A match commits the skipped prefix — the cursor stops before the sync token — and its
  /// hole diagnostic persists; the emission is rewind-safe by construction, because a skip is
  /// committed forward progress and an enclosing rollback unwinds the emission with the log
  /// like any other entry. A resource-limit trip mid-skip commits the skipped prefix at the
  /// durable frontier and returns `Ok(None)` — committed progress, but a failed sync, so **no
  /// hole diagnostic** is emitted for it. A no-match run to end of input commits nothing and
  /// returns `Ok(None)`, leaving no trace: the cursor stays at the pre-call position, the
  /// emissions made during the failed scan (the lexer errors it crossed) are unwound, and the
  /// lexer-error deduplication watermark is restored, so a later genuine consume of the same
  /// region reports its errors exactly once. One diagnostic per hole means **no diagnostic
  /// for a failed hole**. As in [`sync_through`](Self::sync_through), this holds even when
  /// the caller had prefilled the cache with peeked lookahead: a failed sync rewinds the
  /// drained cache prefix too, at the cost of re-lexing those tokens on the next read.
  ///
  /// # Partial mode: an `Incomplete` exit leaves no trace
  ///
  /// Under [`Partial`](crate::input::Partial), a non-final buffer can end mid-scan and this
  /// method surfaces `Incomplete`. That exit commits nothing, so it keeps nothing: the position,
  /// the lexer state, the dedup watermark and every emission the aborted attempt made — each
  /// skipped token's `commit_token` event included — are restored to the call's entry. Refill
  /// and call again and the retry is idempotent; nothing accumulates per attempt.
  ///
  /// # Panic unwind
  ///
  /// A panic out of the predicate, the classifier, the lexer or the emitter **anywhere but the
  /// end-of-input settle** is an exit too, and it settles with **this method's own posture** —
  /// which is neither of the other two's, because this is the method where the family's two axes
  /// cross. It stops *before* the stopping token and commits the skipped prefix, exactly as
  /// [`sync_to`](Self::sync_to) does; it rewinds the full pre-call state at a no-match end of
  /// input, exactly as [`sync_through`](Self::sync_through) does. So the disposition of an unwind
  /// belongs to the **exit**, not to the method, and branching on the method alone is a defect this
  /// crate has already paid for:
  ///
  /// - **once the predicate has answered stop**, in the stop settle itself, the scan keeps the
  ///   prefix its own stop keeps: the position stands at the last skipped token. The in-flight
  ///   token has been handed to that settle by then, so it is **not** put back — the put-back is
  ///   precisely the step the unwind interrupted — and the retained stream is cleared against the
  ///   committed position instead, leaving a catching host's retry to re-lex the swallowed stopper.
  ///   ([`sync_to`](Self::sync_to) states the held-versus-handed-over split in full; this method's
  ///   keeping arm sits entirely on the handed-over side of it.)
  ///   `r9_balanced_stop_exit_panic_keeps_the_prefix_like_its_own_stop_does`
  ///   (`src/input/input_ref/tests.rs`) reads a committed `(3, 5)` over `"ab cd ef gh"`, with four
  ///   lex calls and nothing left live;
  /// - **anywhere else** — mid-scan, and equally once the frontier has left the scope for the very
  ///   `commit_at` that would record it — the scan abandons, to the same restore-to-entry its
  ///   no-match end of input performs: the stream is cleared and the position, the lexer state, the
  ///   dedup watermark and every emission the scan made come back off.
  ///   `r9_frontier_commit_interrupted_abandons_rather_than_half_keeping` reads `((0, 0), 0, 0, 0)`
  ///   — keeping there instead would leave the per-token settles describing a prefix the position
  ///   does not cover, which a retry duplicates.
  ///
  /// Either way the committed position never advances past a token nothing recorded, so no token is
  /// lost and no emitter mark is stranded. Note that the prefix in
  /// question is **skipped, not diagnosed**: this scan makes no per-token report, so the emissions
  /// an unwind decides the fate of are the genuine lexer errors it crossed — the one hole
  /// diagnostic is emitted only after a successful sync returns, which an unwind never reaches.
  ///
  /// The exclusion is the one [`skip_while`](Self::skip_while) states, and it costs this method
  /// nothing: the scope is disarmed before the end-of-input settle runs, but that settle is this
  /// method's *rewind*, and a scan that reaches it has committed no progress for a dropped frontier
  /// to strand — the position was never off the call's entry. It is the stop exit above, not this
  /// one, where the frontier is load-bearing.
  #[inline(always)]
  #[allow(clippy::type_complexity)]
  pub fn sync_balanced<D, F>(
    &mut self,
    mut classifier: D,
    mut pred: F,
  ) -> Result<Option<Hole<L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    D: DelimClass<<L::Token as Token<'inp>>::Kind>,
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
  {
    trace_event!(self, "sync_balanced");
    // A no-match run to end of input must leave no trace — even across a prefilled cache. The scan
    // below skips the cached tokens as readily as the ones it lexes itself, and may cross lexer
    // errors on the way (emitting them and lifting the dedup watermark). Snapshot the pre-call
    // position, the emitter's emission mark, and the watermark HERE — BEFORE the scan — so the
    // end-of-input exit can restore the FULL pre-call state. A match or a limit trip commits the
    // skipped prefix, so this snapshot goes unused on those paths. This is an internal
    // positional rewind, not a `Checkpoint`: it threads no lineage entry.
    //
    // CAPTURE_WINDOW — the watermark clone is hoisted above the mark for the reason
    // [`sync_through`](Self::sync_through) gives: a caller-supplied `L::Offset::clone` evaluated
    // after the capture would be a fallible step between the mark and the `skip_until` call that
    // owns (and settles) it, and an unwind there strands one row of the emitter's checkpoint stack.
    // The `depth`/`skipped`/`first`/`last` bindings and the `decide` closure that sit between the
    // snapshot and `skip_until` are all infallible (scalar zeroes, `None`s, and a closure
    // construction that captures by reference).
    let error_end = self.emitted_error_end.clone();
    // Same window, same reason: the rewind cursor is caller code too, and it is cloned here so
    // that `restore_entry` — which runs with a mark outstanding — performs none.
    let rewind_to = self.span.end_ref().clone();
    let latch = self.latch_snapshot();
    let snapshot = ThroughEntry::new(
      self.span.clone(),
      self.state.clone(),
      self.session.emitter.checkpoint(),
      error_end,
      rewind_to,
      latch,
    );

    let mut depth = 0usize;
    let mut skipped = 0usize;
    let mut first: Option<L::Offset> = None;
    let mut last: Option<L::Offset> = None;

    // The balanced decision, in the seat the plain predicate takes for the rest of the family: sync
    // iff `pred` matches at depth zero; otherwise classify the kind, adjust the depth, and count
    // the token into the hole. The scanner calls it exactly once per token, cached or lexed, so the
    // depth and the count are as blind to the caller's lookahead as the predicate is.
    let mut decide = |tok: Spanned<&L::Token, &L::Span>| -> bool {
      if depth == 0 && pred(tok) {
        return true;
      }
      // UFCS pins the receiver to `L::Token` itself: the blanket `Token` impl for `&'a T`
      // would otherwise tie the closure-local borrow to `'inp`.
      match classifier.classify(&<L::Token as Token<'inp>>::kind(tok.data())) {
        Balance::Open(_) => depth += 1,
        Balance::Close(_) => depth = depth.saturating_sub(1),
        Balance::Neutral => {}
      }
      let span: &L::Span = tok.span();
      if first.is_none() {
        first = Some(span.start_ref().clone());
      }
      last = Some(span.end_ref().clone());
      skipped += 1;
      false
    };

    // `SyncBalanced` stops before the match like `sync_to` — leaving it unconsumed at the cache
    // front — and takes `sync_through`'s no-trace exit at end of input. No per-token diagnostic is
    // made (`REPORT_SKIPPED` is `false`): the one hole note below describes the whole region.
    match self.skip_until::<SyncBalanced, _, _>(&mut decide, || None, snapshot)? {
      Scanned::Found(_) => {}
      Scanned::Exhausted => return Ok(None),
    }

    let hole = match (first, last) {
      (Some(start), Some(end)) => Hole::new(L::Span::new(start, end), skipped),
      // Nothing was skipped: the sync point was the very next token. Record a zero-width hole at
      // the resume position — the cursor, which is the matched token's start, because the scan left
      // that token at the cache front whether it popped it from there or lexed it. No
      // skipped-region diagnostic is emitted for a zero-skip hole.
      _ => {
        let at = self.cursor().as_inner().clone();
        Hole::new(L::Span::new(at.clone(), at), 0)
      }
    };
    // One diagnostic per hole: committed forward progress, reported exactly once. A fatal
    // rejection here propagates with the skip already committed, per the family's fatal-exit
    // discipline.
    if hole.skipped() > 0 {
      self
        .emitter()
        .emit_skipped_region(hole.span_ref().clone(), hole.skipped())?;
    }
    Ok(Some(hole))
  }
}
