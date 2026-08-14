macro_rules! bail {
  ($lib:ident) => {
    use core::marker::PhantomData;

    use $lib::Logos;

    use crate::span::SimpleSpan;

    use super::super::{IntoLexer, Lexer, ReadFrontier, ReadFrontierClass, Source, State, Token};

    /// A trait for token types that can be created from `logos::Logos` types.
    pub trait FromLogos<'inp>: Token<'inp> {
      /// The type which implements `logos::Logos`.
      type Logos: Logos<'inp>;

      /// Converts a `logos::Logos` token into this token type.
      fn from_logos(logos_token: Self::Logos) -> Self;
    }

    impl<'inp, T> FromLogos<'inp> for T
    where
      T: Token<'inp> + Logos<'inp>,
    {
      type Logos = T;

      #[inline(always)]
      fn from_logos(t: Self::Logos) -> Self {
        t
      }
    }

    /// A lexer implementation for [`logos`](https://docs.rs/logos)-based lexers.
    ///
    /// # Limit-error latching
    ///
    /// When the lexer state (`extras`) reports a limit error from
    /// [`check`](Lexer::check) after an item is scanned, [`lex`](Lexer::lex) returns that
    /// error **once** and then latches: every subsequent [`lex`](Lexer::lex) returns
    /// `None`, exactly as if the input were exhausted. This bounds the work an
    /// error-recovery caller performs on untrusted input — once the limiter trips the
    /// lexer stops scanning rather than re-failing on every remaining token.
    ///
    /// **An *item*, not a token.** A `logos` callback is free to update `extras` and return
    /// `Err` for the same matched input, so a scan that lands on a lexer error is a completed
    /// scan whose state must be checked too. When both happen at once the state error wins and
    /// latches; the raw `logos` error is forwarded only when [`check`](Lexer::check) is `Ok`.
    /// A tripped limit is a fact about the accumulated tally, which no later input can clear,
    /// while a lexer error is a fact about one item — the same ranking the input layer applies
    /// when a trip meets an incomplete frontier.
    ///
    /// After latching, [`span`](Lexer::span), [`slice`](Lexer::slice) and
    /// [`bump`](Lexer::bump) continue to delegate to the inner lexer, which is positioned
    /// at the token that tripped the limit; they behave exactly as they do once the input
    /// is exhausted (the latch performs no further scanning).
    ///
    /// Mutating the state through [`state_mut`](Lexer::state_mut) (e.g. resetting the
    /// limiter) does **not** clear the latch: poisoning is sticky for the lifetime of the
    /// lexer instance. A fresh lexer must be constructed to lex again.
    pub struct LogosLexer<'inp, T: FromLogos<'inp>> {
      inner: $lib::Lexer<'inp, T::Logos>,
      poisoned: bool,
      _marker: PhantomData<T>,
    }

    impl<'inp, T, L> IntoLexer<'inp, T> for $lib::Lexer<'inp, L>
    where
      T: FromLogos<'inp, Logos = L> + Token<'inp> + 'inp,
      T::Error: From<L::Error> + From<<L::Extras as State>::Error>,
      L: Logos<'inp> + 'inp,
      L::Extras: State,
      L::Source: Source<usize, Slice<'inp> = <L::Source as $lib::Source>::Slice<'inp>>,
    {
      type Lexer = LogosLexer<'inp, T>;

      #[inline(always)]
      fn into_lexer(self) -> Self::Lexer {
        LogosLexer {
          inner: self,
          poisoned: false,
          _marker: PhantomData,
        }
      }
    }

    impl<'inp, T> LogosLexer<'inp, T>
    where
      T: FromLogos<'inp>,
    {
      /// Consumes the lexer and returns the inner `$lib::Lexer`.
      #[inline(always)]
      pub fn into_inner(self) -> $lib::Lexer<'inp, T::Logos> {
        self.inner
      }

      /// Returns a reference to the inner `$lib::Lexer`.
      #[inline(always)]
      pub const fn inner(&self) -> &$lib::Lexer<'inp, T::Logos> {
        &self.inner
      }

      /// Returns a reference to the inner `$lib::Lexer`.
      #[inline(always)]
      pub const fn inner_mut(&mut self) -> &mut $lib::Lexer<'inp, T::Logos> {
        &mut self.inner
      }
    }

    impl<'inp, T> Lexer<'inp> for LogosLexer<'inp, T>
    where
      T: FromLogos<'inp> + Token<'inp>,
      T::Error: From<<T::Logos as Logos<'inp>>::Error>
        + From<<<T::Logos as Logos<'inp>>::Extras as State>::Error>,
      <T::Logos as Logos<'inp>>::Extras: State + Default,
      <T::Logos as Logos<'inp>>::Source: Source<
          usize,
          Slice<'inp> = <<T::Logos as Logos<'inp>>::Source as $lib::Source>::Slice<'inp>,
        >,
    {
      type State = <T::Logos as Logos<'inp>>::Extras;
      type Source = <T::Logos as Logos<'inp>>::Source;
      type Token = T;
      type Span = SimpleSpan;
      type Offset = usize;

      #[inline(always)]
      fn new(src: &'inp Self::Source) -> Self {
        $lib::Lexer::new(src).into_lexer()
      }

      #[inline(always)]
      fn with_state(src: &'inp Self::Source, state: Self::State) -> Self {
        $lib::Lexer::with_extras(src, state).into_lexer()
      }

      #[inline(always)]
      fn check(&self) -> Result<(), T::Error>
      where
        T: Token<'inp>,
      {
        self.inner.extras.check().map_err(Into::into)
      }

      #[inline(always)]
      fn state(&self) -> &Self::State {
        &self.inner.extras
      }

      #[inline(always)]
      fn state_mut(&mut self) -> &mut Self::State {
        &mut self.inner.extras
      }

      #[inline(always)]
      fn into_state(self) -> Self::State {
        self.inner.extras
      }

      #[inline(always)]
      fn source(&self) -> &'inp Self::Source
      where
        T: Token<'inp>,
      {
        self.inner.source()
      }

      #[inline(always)]
      fn span(&self) -> Self::Span {
        // logos guarantees `start <= end` for every token span, so construct the
        // span directly and skip the checked `From<Range>` -> `new` bounds assert.
        let range = self.inner.span();
        SimpleSpan {
          start: range.start,
          end: range.end,
        }
      }

      #[inline(always)]
      fn slice(&self) -> <Self::Source as Source<Self::Offset>>::Slice<'inp>
      where
        T: Token<'inp>,
      {
        self.inner.slice()
      }

      #[inline(always)]
      fn lex(&mut self) -> Option<Result<T, T::Error>>
      where
        T: Token<'inp>,
      {
        // Once a limit error has been reported, latch: report EOF forever so an
        // error-recovery caller cannot be made to scan the whole input.
        if self.poisoned {
          return None;
        }
        // Drop any value carried in on a restored state, so nothing predating this lexer can be
        // matched against a scan it has not run. It does NOT make the channel per-item: `next()`
        // is not a scan — `logos` resolves `Skip` inside this very call, so what survives it may
        // have been recorded by a scan that produced nothing. `Probe::scanned_from` is what makes
        // the value per-item, and `read_frontier` below is where it is checked.
        //
        // What clearing adds is the case provenance cannot see: an equal start is evidence, and a
        // lexer rebuilt by `with_state` + `bump` can begin its first item at exactly the offset a
        // restored value was keyed to. The two guards fail in different directions and the default
        // impl is an empty `#[inline(always)]` body, so a state that records nothing pays nothing.
        self.inner.extras.clear_probe();
        match self.inner.next() {
          // ONE post-scan check, outside the `Ok`/`Err` split. A logos callback may mutate
          // `extras` and *still* return `Err` for the same matched item, so an item that arrives
          // as a lexer error is a completed scan exactly as a token is. Asking `check` only on
          // the token arm treats it as if nothing had been scanned: a limit trip that coincides
          // with a lexer error is then reported as the raw lexer error, latches nothing, and a
          // recovery loop keeps scanning past the configured limit.
          //
          // The check therefore also *ranks* the two. A tripped state outranks the raw lexer
          // error it arrived with, for the reason it outranks everything else: the tally is
          // monotone and no further input can clear it, while a lexer error is a fact about one
          // item. That is the same precedence the input layer applies one level up, where a
          // tripped limit outranks an incomplete frontier.
          Some(scanned) => match self.check() {
            Ok(()) => Some(scanned.map(T::from_logos).map_err(Into::into)),
            Err(e) => {
              self.poisoned = true;
              Some(Err(e))
            }
          },
          // Untouched: no item was scanned, so there is no post-scan state to rank against.
          None => None,
        }
      }

      /// # Two channels, because `logos` exposes neither the answer nor a way to compute it
      ///
      /// `logos` hands out `span`, `slice` and `remainder`, and nothing at all about where its
      /// DFA probed before settling. So this adapter cannot answer
      /// [`ReadFrontier::SpanEnd`] unconditionally — the engine
      /// **backtracks to the last accepting prefix after probing past it**, so a vocabulary with a
      /// prefix-accepting longer pattern (an integer beside a float or exponent literal) reads
      /// past every short item it emits — and it cannot compute a
      /// [`ReadTo`](crate::ReadFrontier::ReadTo) either. Both answers have to come from the
      /// dialect, and the blanket impl means neither can be an impl the dialect writes:
      ///
      /// - the **class claim**, [`Token::READ_FRONTIER_CLASS`], a const on the vocabulary — the
      ///   same delegation shape as [`Token::SURFACES_TRIVIA`]. It answers for an item whose scan
      ///   recorded nothing, and — unlike that one — it has **no default**, so a vocabulary
      ///   reaching this adapter has stated a class rather than inherited one;
      /// - the **value channel**, [`State::probe`], read off the logos `Extras`. A callback
      ///   that peeks with `remainder()` records how far it looked; that value answers for its own
      ///   item outright, so it must cover the engine's backtracking too, not only the peek.
      ///
      /// # A value is accepted on PROVENANCE, not on freshness
      ///
      /// `next()` is not a scan. `logos` resolves `Skip` **inside** the same call — recursively
      /// through `lex.trivia(); T::lex(lex)` on 0.14/0.15, by `trivia()`-and-continue on 0.16 —
      /// so one call can run several DFA scans and several callbacks before it returns an item.
      /// A trivia callback records for a scan that yields nothing, and the scan that yields the
      /// item may run no callback at all. Reading "there is a value" as "this item recorded it"
      /// hands the trivia's offset to the item, and because the skipped scan *precedes* the item
      /// that offset is **below** the item's real frontier — the one direction that under-reports,
      /// which is the direction that lets an unstable item be committed.
      ///
      /// So the value carries the offset its scan started at
      /// ([`Probe::scanned_from`](crate::Probe::scanned_from)) and is
      /// accepted **iff that equals the returned item's span start**. Anything else — a skipped
      /// scan's value, a value carried in on a restored state — falls back to the class claim,
      /// which is conservative by construction. The check is here rather than in a rule recorders
      /// must follow, because a recorder can only state a fact about the scan it is running in.
      ///
      /// It is asked on the error arm exactly as on the token arm, for the same reason the
      /// post-scan `check()` is: a callback may mutate `extras` and the item still arrive as an
      /// `Err`, so an item that is a lexer error is a completed scan too.
      ///
      /// A recorded value that is behind the item's own span is not repaired here. The driver
      /// floors at `max(span.end, reported)` and that floor is deliberately the *driver's* single
      /// responsibility, so a violating dialect meets one containment, not two that could drift.
      #[inline(always)]
      fn read_frontier(&self) -> ReadFrontier<usize> {
        // `self.inner.span()` is `token_start..token_end` for the item just returned, and a
        // recorder keys on `lexer.span().start` inside its callback. `logos` resets `token_start`
        // at the top of `next()` and again in `trivia()`, so the two are the same quantity read at
        // the same granularity: one DFA scan.
        if let Some(probe) = self.inner.extras.probe()
          && probe.scanned_from() == self.inner.span().start
        {
          return ReadFrontier::ReadTo(probe.probed_to());
        }
        match <T as Token<'inp>>::READ_FRONTIER_CLASS {
          ReadFrontierClass::SpanEnd => ReadFrontier::SpanEnd,
          // Every other class, including any this build does not know, is the conservative
          // answer. `ReadFrontierClass` is `#[non_exhaustive]`, and a variant added later can
          // only ever describe a frontier `Unbounded` already covers.
          _ => ReadFrontier::Unbounded,
        }
      }

      #[inline(always)]
      fn bump(&mut self, n: &usize) {
        self.inner.bump(*n);
      }
    }
  };
}

#[cfg(feature = "logos_0_16")]
pub use self::logos_0_16::{FromLogos, LogosLexer};

#[cfg(all(feature = "logos_0_15", not(feature = "logos_0_16")))]
pub use self::logos_0_15::{FromLogos, LogosLexer};

#[cfg(all(
  feature = "logos_0_14",
  not(any(feature = "logos_0_15", feature = "logos_0_16"))
))]
pub use self::logos_0_14::{FromLogos, LogosLexer};

/// A module containing integrations with the `logos` lexer library version 0.16.
#[cfg(feature = "logos_0_16")]
#[cfg_attr(docsrs, doc(cfg(feature = "logos_0_16")))]
pub mod logos_0_16 {
  bail!(logos_0_16);
}

/// A module containing integrations with the `logos` lexer library version 0.15.
#[cfg(feature = "logos_0_15")]
#[cfg_attr(docsrs, doc(cfg(feature = "logos_0_15")))]
pub mod logos_0_15 {
  bail!(logos_0_15);
}

/// A module containing integrations with the `logos` lexer library version 0.14.
#[cfg(feature = "logos_0_14")]
#[cfg_attr(docsrs, doc(cfg(feature = "logos_0_14")))]
pub mod logos_0_14 {
  bail!(logos_0_14);
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(any(feature = "std", feature = "alloc"))]
#[cfg(any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"))]
mod tests;
