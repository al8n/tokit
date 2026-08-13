use core::convert::Infallible;

/// Trackers for preventing infinite recursion in parsers.
pub mod recursion_tracker;
/// A token tracker for tracking tokens in a lexer.
pub mod token_tracker;
/// A tracker for tracking recursion depth and tokens.
pub mod tracker;

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

  /// The furthest source offset the **scan that produced the most recent item** probed, if that
  /// scan recorded one here.
  ///
  /// This is the per-item **value channel** behind
  /// [`Lexer::read_frontier`](crate::Lexer::read_frontier) for an adapter that cannot see its
  /// engine's probe frontier. The bundled logos adapter is the case it exists for: such a lexer's
  /// `State` *is* the logos `Extras`, which is the one thing a `logos` callback can write to, and
  /// a callback that peeks with `lexer.remainder()` knows exactly how far it looked. Recording it
  /// is what lets `LogosLexer::read_frontier` answer with an offset instead of falling back to the
  /// vocabulary's coarse [`Token::READ_FRONTIER_CLASS`](crate::Token::READ_FRONTIER_CLASS).
  ///
  /// Offsets are `usize` because that is the offset type of every source the logos adapter can
  /// carry. A hand-written lexer implements
  /// [`Lexer::read_frontier`](crate::Lexer::read_frontier) directly and never needs this.
  ///
  /// # It is per-item, and the adapter is what makes that true
  ///
  /// The value must describe **the item just scanned** — a value left over from an earlier item
  /// could be lower than this item's true frontier, and that is the one direction that is
  /// unsound. A recorder is not asked to arrange this: the adapter calls
  /// [`clear_probe`](Self::clear_probe) immediately before each scan, so what
  /// [`probed_to`](Self::probed_to) reports afterwards is exactly what *that* scan recorded, and
  /// `None` when the scan recorded nothing.
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
  /// Offsets are **absolute** in the source, so a callback records
  /// `lexer.span().end + bytes_peeked`, not the peek length.
  ///
  /// # The default, and determinism
  ///
  /// The default is `None` — "this state records nothing" — which degrades to the vocabulary's
  /// class claim and never below it. That is the safe direction: not implementing this costs
  /// precision, never soundness.
  ///
  /// Like everything else a scan makes visible, it must be a pure function of source, offset and
  /// state (see the [determinism clause](crate::Lexer#the-lexer-contract)). Living in the state is
  /// exactly what makes it rewind with a checkpoint restore and reproduce on replay.
  #[inline(always)]
  fn probed_to(&self) -> Option<usize> {
    None
  }

  /// Clears whatever [`probed_to`](Self::probed_to) would report, so the next scan starts from
  /// "nothing recorded".
  ///
  /// The adapter calls this immediately before each scan. It is what makes the value channel
  /// per-item rather than a running high-water mark, and it is why a recorder does not have to
  /// hook the items whose scan runs no callback at all.
  ///
  /// The default is a no-op, matching [`probed_to`](Self::probed_to)'s `None`. A state that
  /// records nothing pays nothing: both are `#[inline(always)]` and empty.
  #[inline(always)]
  fn clear_probe(&mut self) {}
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
