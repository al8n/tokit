use core::convert::Infallible;

/// Trackers for preventing infinite recursion in parsers.
pub mod recursion_tracker;
/// A token tracker for tracking tokens in a lexer.
pub mod token_tracker;
/// A tracker for tracking recursion depth and tokens.
pub mod tracker;

/// A recorded read-frontier value together with **which scan recorded it**.
///
/// The value channel behind [`Lexer::read_frontier`](crate::Lexer::read_frontier) for the
/// bundled logos adapter lives in the lexer state, because the state *is* the `logos` `Extras`
/// and that is the one thing a callback can write to. What a callback cannot do is decide
/// whether the item the adapter is about to return is the item it was called for.
///
/// # One `lex` call is not one scan
///
/// `logos` handles `Skip` **inside** a single `next()` — `lex.trivia(); T::lex(lex)` recursively
/// on 0.14/0.15, a `trivia()`-and-continue loop on 0.16 — so one call can run several DFA scans
/// and several callbacks before it returns an item. A trivia callback records for a scan that
/// produced *nothing*, and the scan that does produce the item may run no callback at all.
///
/// "Recorded during this call" is therefore not "recorded for this item", and the difference is
/// not academic: a skipped scan **precedes** the item, so its offset is *below* the item's true
/// frontier — the one direction that under-reports, and so the one direction that can let an
/// unstable item be committed out of a buffer that is still growing.
///
/// # The key is the scan's start offset
///
/// A recorder gets it for free. Inside a `logos` callback `lexer.span()` is the *current* match,
/// and `logos` resets `token_start` before each rescan, so `lexer.span().start` is always the
/// scan that is running right now. After `next()` returns, the adapter has the returned item's
/// span. It accepts the value **iff the two starts are equal** and otherwise answers from
/// [`Token::SCAN_LOOKAHEAD`](crate::Token::SCAN_LOOKAHEAD), which is conservative by
/// construction.
///
/// That check lives in the adapter, not in an obligation on recorders: a recorder states a fact
/// about the scan it is running in, and nothing it can get wrong turns into a frontier the
/// driver trusts for a different item.
///
/// ```
/// use tokora::Probe;
///
/// // A callback that matched `[ \t]+` at 3..5 and then inspected the two bytes at 5 and 6.
/// // `span.end` is 5 — the first offset OUTSIDE the match — so two bytes from there reach 6,
/// // not 7. The value is the highest offset touched, inclusive.
/// let probe = Probe::new(3, 6);
/// assert_eq!(probe.scanned_from(), 3);
/// assert_eq!(probe.probed_to(), 6);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Probe {
  scanned_from: usize,
  probed_to: usize,
}

impl Probe {
  /// Records that a scan beginning at `scanned_from` probed the input as far as `probed_to`,
  /// **inclusive**, both absolute offsets in the source.
  ///
  /// A probe answered by end of input counts at the offset probed — the same probe-inclusive
  /// convention [`ReadFrontier`](crate::ReadFrontier) documents.
  ///
  /// # The arithmetic: `- 1`, because `span.end` is already outside the match
  ///
  /// A span is half-open, so `lexer.span().end` is the **first offset the match does not
  /// cover** — the offset lookahead starts at, not the last one the match touched. A callback
  /// that successfully inspects `n` bytes from there touches
  /// `span().end ..= span().end + n - 1`, so for `n > 0` it records
  ///
  /// ```text
  /// lexer.span().end + n - 1
  /// ```
  ///
  /// and `span().end + n` **only** when the scan also reached for the byte at that offset and
  /// found end of input — because an attempted-and-absent byte counts at the offset it was
  /// wanted. `n == 0` — no lookahead at all beyond the match — records nothing above
  /// `span().end - 1`; a scan that consulted the terminator byte at `span().end` records
  /// `span().end`, which is [`SpanEnd`](crate::ReadFrontier::SpanEnd)'s one-boundary-byte
  /// allowance spelled as an offset.
  ///
  /// Getting this wrong by one is not cosmetic in the safe direction only. Following
  /// `span().end + n` makes a callback that read the **real** last byte of the buffer report the
  /// offset one past it — end of input's offset — so the driver's `frontier >= len` predicate
  /// withholds an item that was append-stable, and under a
  /// [`Budget`](crate::input::Budget) the re-driven attempts that withholding costs can exhaust
  /// the session.
  #[inline(always)]
  pub const fn new(scanned_from: usize, probed_to: usize) -> Self {
    Self {
      scanned_from,
      probed_to,
    }
  }

  /// Returns the offset the recording scan started at — `lexer.span().start` inside the
  /// callback that recorded it.
  ///
  /// This is the provenance the adapter matches against the returned item's span start. It is
  /// not a frontier and is never reported as one.
  #[inline(always)]
  pub const fn scanned_from(&self) -> usize {
    self.scanned_from
  }

  /// Returns the furthest offset that scan probed, inclusive.
  #[inline(always)]
  pub const fn probed_to(&self) -> usize {
    self.probed_to
  }
}

/// The state trait for lexers
pub trait State: core::fmt::Debug + Clone {
  /// The error type of the state.
  type Error: Clone;

  /// Checks the state for errors.
  ///
  /// Not to be confused with [`Check::check`](crate::Check::check) (the parser-side value
  /// predicate) or [`Lexer::check`](crate::Lexer::check) (the lexer-level probe that
  /// typically wraps this one into the token's error type).
  fn check(&self) -> Result<(), Self::Error>;

  /// Takes the [`Probe`] the most recent scan recorded here, leaving nothing behind.
  ///
  /// This is the **value channel** behind [`Lexer::read_frontier`](crate::Lexer::read_frontier)
  /// for an adapter that cannot see its engine's probe frontier. The bundled logos adapter is the
  /// case it exists for: such a lexer's `State` *is* the logos `Extras`, which is the one thing a
  /// `logos` callback can write to, and a callback that peeks with `lexer.remainder()` knows
  /// exactly how far it looked. Recording it is what lets `LogosLexer::read_frontier` answer with
  /// an offset instead of falling back to the vocabulary's coarse
  /// [`Token::SCAN_LOOKAHEAD`](crate::Token::SCAN_LOOKAHEAD).
  ///
  /// Offsets are `usize` because that is the offset type of every source the logos adapter can
  /// carry. A hand-written lexer implements
  /// [`Lexer::read_frontier`](crate::Lexer::read_frontier) directly and never needs this.
  ///
  /// # Reading IS clearing, and that is the point
  ///
  /// The channel needs two things of a state: hand the value over, and be empty before the next
  /// scan runs. Those were once two defaulted members, and the pair could be half-implemented —
  /// override the reader, inherit the empty reset, and the state hands out a value nothing ever
  /// removes. That is not a coarser answer but a wrong one, because a recorded value **answers
  /// the frontier contract outright**: a value predating the lexer entirely wins over the
  /// vocabulary's honest [`Unbounded`](crate::ScanLookahead::Unbounded), and the driver's
  /// `max(span.end, reported)` floor can leave it below a growing buffer's length, releasing an
  /// item whose scan really did reach the buffer end.
  ///
  /// One consuming operation cannot be half-implemented: there is no sibling to forget, and a
  /// state that implements nothing records nothing. Taking is therefore part of the contract, not
  /// an optimisation — an implementation that returns the value without removing it re-offers,
  /// on the next scan, a value that scan did not record.
  ///
  /// The adapter calls this **twice** per `lex`, and both calls are this one method: once
  /// immediately before the scan, discarding whatever came in on a restored state, and once
  /// after, capturing what the scan recorded.
  ///
  /// # It answers for ONE scan, and the recorded start is what says which
  ///
  /// A value left over from an earlier scan could be lower than this item's true frontier, and
  /// that is the one direction that is unsound. A recorder is not asked to police that: it
  /// records [`Probe::scanned_from`] — `lexer.span().start`, the scan it is running in — and the
  /// **adapter** accepts the value only for the item that scan produced. Everything else falls
  /// back to the class claim.
  ///
  /// That check is why one `lex` call may record several times without confusing anything:
  /// `logos` resolves `Skip` inside a single `next()`, so a trivia callback records for a scan
  /// that yields nothing, and the scan that does yield the item may run no callback at all. See
  /// [`Probe`] for the mechanism.
  ///
  /// The pre-scan take is a second, independent guard: provenance rejects a stale value by
  /// mismatch, and taking removes it, so a rebuilt lexer positioned by
  /// [`bump`](crate::Lexer::bump) cannot start an item at exactly the offset a restored state's
  /// value was keyed to.
  ///
  /// # What a recorded value must cover
  ///
  /// A recorded value **answers the frontier contract for that item outright** — the class claim
  /// is not consulted for it. So it must cover *everything* the item's decision depended on,
  /// including any probe the engine's own prefix backtracking performed, not only what the
  /// callback itself read. Recording just the callback's peek for an item the DFA also backtracked
  /// over under-reports, which is a contract violation the `conformance` kit's `run_partial` check
  /// is there to catch.
  ///
  /// That burden is what covers the case where `logos` runs a callback, backtracks, and settles
  /// on a **different candidate beginning at the same offset**: the starts match, so the value is
  /// accepted, and it may be the rejected candidate's. It is still the recorder's job to make it
  /// an upper bound on everything deciding the item read — and `run_partial` still falsifies a
  /// value that is not.
  ///
  /// Offsets are **absolute** in the source and the value is the highest offset touched,
  /// **inclusive** — so a callback that inspected `n` bytes past its match records
  /// `lexer.span().end + n - 1`, not the peek length and not `span().end + n`. See
  /// [`Probe::new`] for the whole rule, including the one case where `+ n` is the right answer.
  ///
  /// # The default, and determinism
  ///
  /// The default is `None` — "this state records nothing" — which degrades to the vocabulary's
  /// class claim and never below it. That is the safe direction: not implementing this costs
  /// precision, never soundness. A state that records nothing pays nothing: the default is an
  /// empty `#[inline(always)]` body.
  ///
  /// Like everything else a scan makes visible, it must be a pure function of source, offset and
  /// state (see the [determinism clause](crate::Lexer#the-lexer-contract)). Living in the state is
  /// exactly what makes it rewind with a checkpoint restore and reproduce on replay.
  #[inline(always)]
  fn take_probe(&mut self) -> Option<Probe> {
    None
  }
}

impl State for () {
  type Error = ();

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    Ok(())
  }
}

impl State for Infallible {
  type Error = Infallible;

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    Ok(())
  }
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(feature = "std")]
mod tests;
