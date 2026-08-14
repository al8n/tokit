use crate::{
  Token,
  error::{MaybeIncomplete, MaybeTerminal, UnexpectedEot},
  input::DelimClass,
  span::Span as _,
};

use super::{
  recovery_gate::{Attempted, Stepped, recovery_step, speculated_attempt},
  *,
};

/// A recovery combinator that skips to a synchronization point and retries the inner parser.
///
/// On failure of the inner parser, the input is rolled back to where the attempt began (the
/// failed attempt leaves no trace), then
/// [`sync_balanced`](InputRef::sync_balanced) skips forward — nesting-aware, using the held
/// [`DelimClass`] classifier and depth-0 sync predicate — and the inner parser runs again from
/// the sync point. Each successful skip is committed forward progress described by exactly one
/// skipped-region diagnostic (see [`Hole`](crate::input::Hole)); if an enclosing
/// [`attempt`](InputRef::attempt) or transaction rolls the whole recovery back, those
/// emissions unwind with the log like any other entry.
///
/// # The retry loop and the progress guard
///
/// A retry *cycle* is: sync to the next depth-0 sync point, then re-run the inner parser.
/// Cycles repeat while they make progress:
///
/// - a retry that **succeeds** ends the loop with its value;
/// - a cycle that **consumes nothing** — the sync point was already at hand and the retry
///   failed without net consumption — bails out with the error that triggered it (for the
///   first cycle, the original error), so a zero-consumption cycle can never loop;
/// - a retry that fails after real progress records its error as the next cycle's trigger and
///   **consumes the sync token** before re-syncing — that sync point did not admit a
///   successful parse, so the next cycle scans strictly past it. Every continuing cycle
///   therefore consumes at least one token, and the loop terminates: at the latest, a sync
///   that finds no further sync point (end of input, which itself leaves no trace) surfaces
///   the last recorded error.
///
/// # The never-recoverable law, and its terminal dual
///
/// An [`Incomplete`](crate::error::Incomplete) error, and a **terminal stop**, are re-raised
/// untouched — before any skip, and from any retry — exactly as [`Recover`] does, off the same five
/// witnesses it documents. Recovery synthesizes progress over a *malformed* construct: an
/// incomplete one is merely unfinished, so skipping would drop input that has not finished
/// arriving, and no quantity of skipped input un-trips a resource limit. See [`MaybeIncomplete`]
/// and [`MaybeTerminal`].
///
/// Each retry cycle is judged as its own attempt, against its own baselines. That is what keeps an
/// ordinary syntax error in cycle *n* recoverable after cycle *n − 1* legitimately caught and
/// parsed past a budget — the session counter is monotone, so a baseline shared across cycles would
/// refuse every retry after the parse's first stop.
///
/// ## The skip and the advance are covered too, and they are where the law used to leak
///
/// The law is about *recovery*, and this combinator's recovery is not only its retries: it is the
/// **skip** to a sync point and the **advance** over a sync point that did not admit a parse.
/// Neither is a parse attempt, and both used to run outside the gate entirely.
///
/// Both are `Ok`-returning primitives that fold a terminal trip into the value they use for
/// genuine exhaustion — [`sync_balanced`](InputRef::sync_balanced) answers `Ok(None)` whether it
/// found no sync point or the scanner tripped mid-skip, and [`next`](InputRef::next) answers
/// `Ok(None)` whether the input ended or the scan tripped. So a spent scanner budget read as
/// *"nothing left to skip to"*, and the combinator surfaced its ordinary trigger error for it.
/// Through a **rejecting** emitter that never showed, because the rejection propagates as an `Err`
/// from the skip itself; through an **accepting** one the trip's diagnostic is filed and the `Ok`
/// comes back looking exactly like end of input.
///
/// The witnesses are now sampled across both, and a stop inside either surfaces the same
/// terminal-marked [`UnexpectedEot`](crate::error::UnexpectedEot) every other committed exit in
/// this crate surfaces for a trip an accepting emitter took — so *"the scanner stopped"* and
/// *"there was nowhere to sync to"* are two different answers again, and only the second is
/// recoverable.
///
/// # Example
///
/// ```ignore
/// use tokora::{ParseInput, input::Balance};
///
/// // Parse a statement; on failure skip (nesting-aware) to the next `;` and retry.
/// let parser = parse_statement().skip_then_retry(
///     |kind: &TokenKind| match kind {
///         TokenKind::LBrace => Balance::Open('{'),
///         TokenKind::RBrace => Balance::Close('{'),
///         _ => Balance::Neutral,
///     },
///     |tok| matches!(tok.data(), Token::Semi),
/// );
/// // Input: "### ; let x = 1;" → skips `###` (one hole), retries at `;`…
/// ```
///
/// # See Also
///
/// - [`sync_balanced`](InputRef::sync_balanced) — the skip primitive and its contract
/// - [`Recover`] — recovery by an alternative parser from the original position
/// - [`InplaceRecover`] — recovery continuing from the error position
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SkipThenRetry<P, D, F, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  classifier: D,
  pred: F,
  _m: PhantomData<O>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _l: PhantomData<L>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, D, F, O, L, Ctx, Lang: ?Sized, Cmpl> SkipThenRetry<P, D, F, O, L, Ctx, Lang, Cmpl> {
  /// Creates a new `SkipThenRetry` parser.
  #[inline(always)]
  pub(crate) const fn new(parser: P, classifier: D, pred: F) -> Self {
    Self {
      parser,
      classifier,
      pred,
      _m: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _l: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, D, F, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for SkipThenRetry<P, D, F, O, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  D: DelimClass<<L::Token as Token<'inp>>::Kind>,
  F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  // `From<UnexpectedEot<..>>` is what lets the skip and the advance report a terminal stop the way
  // every other committed exit in this crate reports one. It is the same conversion
  // `next_or_stop`, `try_expect_or_stop`, the peek family and both pratt engines already require —
  // the crate's universal end-of-input carrier, and one fifth of `FromTokenErrors` — so a grammar
  // that can reach end of input at all already has it.
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    MaybeIncomplete + MaybeTerminal + From<UnexpectedEot<L::Offset, Lang>>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // First attempt, exactly `Recover`'s shape and through the same chokepoint: speculate so a
    // failure rolls back to the pre-parse state (position, lexer state, emissions), and re-raise an
    // `Incomplete` — or a terminal stop — untouched before any skip, since no skipping clears
    // either (the never-recoverable law and its terminal dual).
    //
    // Terminality is read from all five places it is stored, and this is the arm where the
    // difference cost committed input: through a sink that does not carry it, the cycles below used
    // to skip the whole tripping construct and its sync token, buying progress that cannot help,
    // because no quantity of skipped input makes the next descent shallower or un-trips a scanner
    // budget. See [`recovery_gate`](super::recovery_gate) for the five witnesses and their
    // baselines.
    let mut err = match speculated_attempt(inp, |input| self.parser.parse_input(input)) {
      Attempted::Done(output) => return Ok(output),
      Attempted::Reraise(e) => return Err(e),
      Attempted::Recoverable(e) => e,
    };

    loop {
      // The cycle's progress anchor: the committed position before this sync.
      let before = inp.cursor().as_inner().clone();

      // Skip to the next depth-0 sync point, THROUGH THE CHOKEPOINT. The classifier is re-borrowed
      // through a closure (any `DelimClass` is reusable across cycles that way); a fatal emitter
      // rejection mid-skip propagates per the sync family's fatal-exit discipline.
      //
      // The skip is recovery work, so it is gated like one — see
      // [`recovery_gate`](super::recovery_gate)'s "the work outside an attempt". `sync_balanced`
      // answers `Ok(None)` both for "no sync point before end of input" (which leaves no trace,
      // and for which this cycle's trigger error is the right thing to surface: there is nowhere
      // left to retry from) and for "the scanner tripped mid-skip" (which commits the skipped
      // prefix at the durable frontier and is a terminal stop). The step tells them apart.
      let classifier = &mut self.classifier;
      let pred = &mut self.pred;
      let synced = recovery_step(inp, |input| {
        input.sync_balanced(
          |kind: &<L::Token as Token<'inp>>::Kind| classifier.classify(kind),
          pred,
        )
      })?;
      match synced {
        // A terminal stop inside the skip. No quantity of further skipping clears it, so this is
        // not "nowhere to sync to" — it is the parse stopping, and it says so in the carrier every
        // other committed exit uses for a trip an accepting emitter took.
        Stepped::Stopped => return Err(terminal_stop(inp)),
        // Genuine exhaustion: no sync point before the end of input, and the skip left no trace.
        // This cycle's trigger error is the answer — there is nowhere left to retry from.
        Stepped::Went(found) => {
          if found.is_none() {
            return Err(err);
          }
        }
      }

      // Each retry cycle is its own attempt, and the chokepoint takes its baselines per call — so
      // this cycle's are taken here, on this iteration, and a stop an earlier cycle caused is
      // already an `Err` this loop returned rather than something to charge to this one. That
      // per-cycle granularity used to be a hand-written `trip_snapshot()` inside the loop, load
      // bearing and unguarded: hoisted out, the monotone session counter would make every retry
      // after the parse's first trip re-raise. There is no longer a baseline here to hoist.
      match speculated_attempt(inp, |input| self.parser.parse_input(input)) {
        Attempted::Done(output) => return Ok(output),
        // The law applies to every raise: an `Incomplete`, a terminal scanner stop in either of the
        // places it is stored, or a resource budget trip this retry caused re-raises unchanged,
        // with no further skipping.
        Attempted::Reraise(e) => return Err(e),
        Attempted::Recoverable(e) => {
          // The progress guard: a cycle that consumed nothing — zero-skip sync, and the
          // failed retry rolled back to the same spot — must not loop. Bail with the error
          // that triggered this cycle (for the first cycle, the original error).
          if *inp.cursor().as_inner() <= before {
            return Err(err);
          }
          err = e;
          // This sync point did not admit a successful retry: consume it so the next cycle
          // scans strictly past it — the guarantee that every continuing cycle consumes at
          // least one token. Through the chokepoint for the same reason the skip is: `next`
          // folds a fresh trip into the same `Ok(None)` it uses for genuine exhaustion, so
          // "nothing left to consume" and "the scanner stopped" arrived as one value.
          match recovery_step(inp, InputRef::next)? {
            Stepped::Stopped => return Err(terminal_stop(inp)),
            Stepped::Went(consumed) => {
              if consumed.is_none() {
                return Err(err);
              }
            }
          }
        }
      }
    }
  }
}

/// The terminal stop a sync or advance step ran into, in the carrier this crate uses for one.
///
/// Byte for byte what [`next_or_stop`](InputRef::next_or_stop) and
/// [`try_expect_or_stop`](InputRef::try_expect_or_stop) build on their `Tripped` arms, and what
/// `parser::many::absence_after_element` builds when a collection's absence gate sees a stop — an
/// end-of-input error at the committed position, marked terminal. So a trip an accepting emitter
/// filed reaches the caller the same way from a recovery phase as it does from a leaf or from a
/// collection, which is the whole of what "terminality is a property of the event" buys. A
/// *rejecting* emitter never reaches here: its rejection is an `Err` out of the step itself.
///
/// The `From<UnexpectedEot<..>>` bound this needs is the one that absence gate already carries, and
/// the one every `_or_stop` primitive in the crate carries. There is no bound-free way to do this:
/// [`MaybeTerminal`] is a *predicate*, with nothing to construct from, and leaving the stop
/// unmarked is the defect rather than the alternative to it.
#[inline(always)]
fn terminal_stop<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
  inp: &InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
) -> <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
{
  UnexpectedEot::eot_of(inp.span().end())
    .into_terminal()
    .into()
}

// ── RECOVERY_GATE_CENSUS_END — production above, tests below ──────────────────
//
// `RECOVERY_GATE_CENSUS` (in `parser/recovery_gate.rs`) reads this file's source and counts
// needles that its own test fixtures also spell. The split is this marker rather than the first
// `#[cfg(test)]`, because the suites below are feature-gated and their attribute is not that
// literal; the census `expect()`s the marker, so deleting it fails loudly instead of silently
// widening the scan over test code.

// Recovery behavior needs a lexer that actually runs, which pins the suite to `logos` + `std` —
// the same gate as the `Recover` tests.
#[cfg(all(test, feature = "logos_0_16", feature = "std"))]
mod tests {
  use super::*;
  use crate::{
    Emitter, ParseContext, Token, cache::DefaultCache, emitter::Verbose,
    error::token::UnexpectedToken, input::Balance, input::Input, lexer::LogosLexer,
    span::SimpleSpan,
  };
  use core::cell::Cell;
  use std::{rc::Rc, vec};

  #[derive(Debug, Clone, PartialEq)]
  enum RtErr {
    Primary,
    Retry,
    Incomplete,
    Terminal,
    Lex,
  }

  impl From<()> for RtErr {
    fn from(_: ()) -> Self {
      RtErr::Lex
    }
  }

  impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for RtErr {
    fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
      RtErr::Lex
    }
  }

  // The construction path W5's frontier rules use (`SurfaceIncomplete` → `Error::from(Incomplete)`),
  // kept coherent with `is_incomplete()` below so the never-recoverable law holds for the value the
  // input layer actually surfaces.
  impl From<crate::error::Incomplete<usize>> for RtErr {
    fn from(_: crate::error::Incomplete<usize>) -> Self {
      RtErr::Incomplete
    }
  }

  impl crate::error::MaybeIncomplete for RtErr {
    fn is_incomplete(&self) -> bool {
      matches!(self, RtErr::Incomplete)
    }
  }

  // The terminal twin: a terminal-marked end-of-input value (the way `try_expect_or_stop` and the
  // delimited close build one) maps to `Terminal`, kept coherent with `is_terminal()` so the
  // terminal dual of the never-recoverable law holds for the value the input layer surfaces.
  impl From<crate::error::UnexpectedEot<usize>> for RtErr {
    fn from(e: crate::error::UnexpectedEot<usize>) -> Self {
      if e.is_terminal() {
        RtErr::Terminal
      } else {
        RtErr::Lex
      }
    }
  }

  impl crate::error::MaybeTerminal for RtErr {
    fn is_terminal(&self) -> bool {
      matches!(self, RtErr::Terminal)
    }
  }

  #[derive(Debug, Clone, PartialEq, Eq, crate::logos::Logos)]
  #[logos(crate = crate::logos, skip r"[ \t\r\n]+")]
  enum RtTok {
    #[regex(r"[0-9]+")]
    Num,
    #[regex(r"[a-z]+")]
    Ident,
    #[token(";")]
    Semi,
  }

  impl core::fmt::Display for RtTok {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str(match self {
        Self::Num => "number",
        Self::Ident => "identifier",
        Self::Semi => "`;`",
      })
    }
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  enum RtKind {
    Num,
    Ident,
    Semi,
  }

  impl core::fmt::Display for RtKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str(match self {
        Self::Num => "number",
        Self::Ident => "identifier",
        Self::Semi => "`;`",
      })
    }
  }

  impl Token<'_> for RtTok {
    type Kind = RtKind;
    type Error = RtErr;

    const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

    fn kind(&self) -> RtKind {
      match self {
        Self::Num => RtKind::Num,
        Self::Ident => RtKind::Ident,
        Self::Semi => RtKind::Semi,
      }
    }

    fn is_trivia(&self) -> bool {
      false
    }
  }

  type Lex<'a> = LogosLexer<'a, RtTok>;
  type Ctx<'a> = (Verbose<RtErr>, DefaultCache<'a, Lex<'a>>);
  type EmitErr<'a> =
    <<Ctx<'a> as ParseContext<'a, Lex<'a>, ()>>::Emitter as Emitter<'a, Lex<'a>, ()>>::Error;

  /// No pairs in this grammar: every kind is neutral.
  fn neutral(_: &RtKind) -> Balance<()> {
    Balance::Neutral
  }

  /// The depth-0 sync predicate used across the tests unless stated otherwise.
  fn is_num(t: Spanned<&RtTok, &SimpleSpan>) -> bool {
    matches!(t.data(), RtTok::Num)
  }

  /// Consumes one token and requires a number; any other outcome is a `Primary` failure
  /// (the combinator's attempt rolls the consumption back).
  struct NumParser;

  impl<'inp> ParseInput<'inp, Lex<'inp>, (), Ctx<'inp>, ()> for NumParser {
    fn parse_input(
      &mut self,
      inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>, ()>,
    ) -> Result<(), EmitErr<'inp>> {
      match inp.next()? {
        Some(t) if matches!(t.data(), RtTok::Num) => Ok(()),
        _ => Err(RtErr::Primary),
      }
    }
  }

  /// Fails on every application without consuming: `Primary` first, `Retry` afterwards. The
  /// shared counter observes how many times the combinator applied it.
  struct SeqFail {
    calls: Rc<Cell<usize>>,
  }

  impl<'inp> ParseInput<'inp, Lex<'inp>, (), Ctx<'inp>, ()> for SeqFail {
    fn parse_input(
      &mut self,
      _inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>, ()>,
    ) -> Result<(), EmitErr<'inp>> {
      self.calls.set(self.calls.get() + 1);
      Err(if self.calls.get() == 1 {
        RtErr::Primary
      } else {
        RtErr::Retry
      })
    }
  }

  /// Fails outright with a chosen error, without consuming; counts its applications.
  struct FailWith {
    err: RtErr,
    calls: Rc<Cell<usize>>,
  }

  impl<'inp> ParseInput<'inp, Lex<'inp>, (), Ctx<'inp>, ()> for FailWith {
    fn parse_input(
      &mut self,
      _inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>, ()>,
    ) -> Result<(), EmitErr<'inp>> {
      self.calls.set(self.calls.get() + 1);
      Err(self.err.clone())
    }
  }

  #[test]
  fn skip_then_retry_succeeds_after_one_hole() {
    //   a b 1
    //   0 2 4
    // The primary fails on `a`; the sync skips `a b` (one hole, two tokens) and the retry
    // parses `1`.
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::with_state_and_context(
      "a b 1",
      (),
      crate::input::InputContext::new(
        Verbose::<RtErr>::new(),
        DefaultCache::<'_, Lex<'_>>::default(),
      ),
    );
    {
      let mut inp = input.as_ref();

      let mut p =
        SkipThenRetry::<_, _, _, (), Lex<'_>, Ctx<'_>, ()>::new(NumParser, neutral, is_num);
      assert_eq!(
        p.parse_input(&mut inp),
        Ok(()),
        "the retry parses the number"
      );
      assert_eq!(inp.span(), &SimpleSpan::new(4, 5), "the number is consumed");
    }

    assert_eq!(
      input
        .emitter()
        .skipped_regions()
        .get(&SimpleSpan::new(0, 3)),
      Some(&vec![2usize]),
      "exactly one hole: the skipped `a b`"
    );
    let total: usize = input.emitter().errors().values().map(|g| g.len()).sum();
    assert_eq!(total, 0, "no per-token diagnostics from the recovery skip");
  }

  #[test]
  fn skip_then_retry_zero_consumption_cycle_bails_with_original_error() {
    // The pinned progress guard: the sync point is immediately at hand (zero-skip) and the
    // retry fails without consuming — the cycle consumed nothing, so the combinator bails
    // out with the ORIGINAL error rather than looping.
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::with_state_and_context(
      "1 2",
      (),
      crate::input::InputContext::new(
        Verbose::<RtErr>::new(),
        DefaultCache::<'_, Lex<'_>>::default(),
      ),
    );
    let calls = Rc::new(Cell::new(0));
    {
      let mut inp = input.as_ref();

      let mut p = SkipThenRetry::<_, _, _, (), Lex<'_>, Ctx<'_>, ()>::new(
        SeqFail {
          calls: calls.clone(),
        },
        neutral,
        |_t: Spanned<&RtTok, &SimpleSpan>| true,
      );
      assert_eq!(
        p.parse_input(&mut inp),
        Err(RtErr::Primary),
        "the zero-consumption cycle surfaces the original error, not the retry's"
      );
      assert_eq!(inp.span(), &SimpleSpan::new(0, 0), "no progress committed");
    }

    assert_eq!(calls.get(), 2, "exactly one retry ran before the bailout");
    let holes: usize = input
      .emitter()
      .skipped_regions()
      .values()
      .map(|g| g.len())
      .sum();
    assert_eq!(holes, 0, "a zero-skip sync reports no hole");
    let total: usize = input.emitter().errors().values().map(|g| g.len()).sum();
    assert_eq!(total, 0, "the bailed-out recovery leaves no diagnostics");
  }

  #[test]
  fn skip_then_retry_reraises_incomplete_without_skipping() {
    // The never-recoverable law: an `Incomplete` passes through untouched — the classifier
    // and the sync predicate are never consulted, nothing is skipped, nothing is emitted.
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::with_state_and_context(
      "1 2 3",
      (),
      crate::input::InputContext::new(
        Verbose::<RtErr>::new(),
        DefaultCache::<'_, Lex<'_>>::default(),
      ),
    );
    let calls = Rc::new(Cell::new(0));
    let classified = Cell::new(false);
    let synced = Cell::new(false);
    {
      let mut inp = input.as_ref();

      let mut p = SkipThenRetry::<_, _, _, (), Lex<'_>, Ctx<'_>, ()>::new(
        FailWith {
          err: RtErr::Incomplete,
          calls: calls.clone(),
        },
        |_k: &RtKind| {
          classified.set(true);
          Balance::<()>::Neutral
        },
        |_t: Spanned<&RtTok, &SimpleSpan>| {
          synced.set(true);
          false
        },
      );
      assert_eq!(
        p.parse_input(&mut inp),
        Err(RtErr::Incomplete),
        "an Incomplete is re-raised untouched on the Err channel"
      );
      assert_eq!(inp.span(), &SimpleSpan::new(0, 0), "the input is untouched");
    }

    assert_eq!(
      calls.get(),
      1,
      "the parser ran once; no retry for an Incomplete"
    );
    assert!(
      !classified.get(),
      "the classifier never runs for an Incomplete"
    );
    assert!(
      !synced.get(),
      "the sync predicate never runs for an Incomplete"
    );
    let holes: usize = input
      .emitter()
      .skipped_regions()
      .values()
      .map(|g| g.len())
      .sum();
    assert_eq!(holes, 0, "nothing was skipped");
    let total: usize = input.emitter().errors().values().map(|g| g.len()).sum();
    assert_eq!(total, 0, "nothing was emitted");
  }

  #[test]
  fn skip_then_retry_reraises_a_from_incomplete_built_error() {
    // The value W5's frontier rules surface is built via `From<Incomplete>`. Prove that value is
    // recognized as incomplete and re-raised before any skip — the classifier and sync predicate
    // never run, nothing is emitted.
    let surfaced: RtErr = crate::error::Incomplete::new(4usize).into();
    assert!(surfaced.is_incomplete());

    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::with_state_and_context(
      "1 2 3",
      (),
      crate::input::InputContext::new(
        Verbose::<RtErr>::new(),
        DefaultCache::<'_, Lex<'_>>::default(),
      ),
    );
    let calls = Rc::new(Cell::new(0));
    let classified = Cell::new(false);
    let synced = Cell::new(false);
    {
      let mut inp = input.as_ref();
      let mut p = SkipThenRetry::<_, _, _, (), Lex<'_>, Ctx<'_>, ()>::new(
        FailWith {
          err: surfaced.clone(),
          calls: calls.clone(),
        },
        |_k: &RtKind| {
          classified.set(true);
          Balance::<()>::Neutral
        },
        |_t: Spanned<&RtTok, &SimpleSpan>| {
          synced.set(true);
          false
        },
      );
      assert_eq!(
        p.parse_input(&mut inp),
        Err(surfaced),
        "a From<Incomplete>-built error is re-raised untouched"
      );
    }
    assert_eq!(calls.get(), 1, "the parser ran once; no retry");
    assert!(!classified.get(), "the classifier never runs");
    assert!(!synced.get(), "the sync predicate never runs");
    let holes: usize = input
      .emitter()
      .skipped_regions()
      .values()
      .map(|g| g.len())
      .sum();
    assert_eq!(holes, 0, "nothing was skipped");
  }

  #[test]
  fn skip_then_retry_reraises_a_terminal_stop_before_any_skip() {
    // The terminal dual: a terminal scanner stop is not a malformed construct — skipping to a sync
    // point and retrying only re-trips the same limit — so it must re-raise before any skip, exactly
    // as an Incomplete does. The value is built via `From<UnexpectedEot>` with the terminal marker
    // set (the way `try_expect_or_stop` builds it); the classifier and sync predicate never run and
    // nothing is skipped.
    let surfaced: RtErr = crate::error::UnexpectedEot::eot_of(4usize)
      .into_terminal()
      .into();
    assert_eq!(surfaced, RtErr::Terminal);
    assert!(surfaced.is_terminal());

    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::with_state_and_context(
      "1 2 3",
      (),
      crate::input::InputContext::new(
        Verbose::<RtErr>::new(),
        DefaultCache::<'_, Lex<'_>>::default(),
      ),
    );
    let calls = Rc::new(Cell::new(0));
    let classified = Cell::new(false);
    let synced = Cell::new(false);
    {
      let mut inp = input.as_ref();
      let mut p = SkipThenRetry::<_, _, _, (), Lex<'_>, Ctx<'_>, ()>::new(
        FailWith {
          err: surfaced.clone(),
          calls: calls.clone(),
        },
        |_k: &RtKind| {
          classified.set(true);
          Balance::<()>::Neutral
        },
        |_t: Spanned<&RtTok, &SimpleSpan>| {
          synced.set(true);
          false
        },
      );
      assert_eq!(
        p.parse_input(&mut inp),
        Err(surfaced),
        "a terminal stop is re-raised untouched, before any skip"
      );
    }
    assert_eq!(
      calls.get(),
      1,
      "the parser ran once; no retry for a terminal stop"
    );
    assert!(
      !classified.get(),
      "the classifier never runs for a terminal stop"
    );
    assert!(
      !synced.get(),
      "the sync predicate never runs for a terminal stop"
    );
    let holes: usize = input
      .emitter()
      .skipped_regions()
      .values()
      .map(|g| g.len())
      .sum();
    assert_eq!(holes, 0, "nothing was skipped");
  }

  #[test]
  fn skip_then_retry_failed_sync_surfaces_the_error() {
    // No sync point before end of input: the failed sync leaves no trace and the original
    // error surfaces.
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::with_state_and_context(
      "a b",
      (),
      crate::input::InputContext::new(
        Verbose::<RtErr>::new(),
        DefaultCache::<'_, Lex<'_>>::default(),
      ),
    );
    {
      let mut inp = input.as_ref();

      let mut p =
        SkipThenRetry::<_, _, _, (), Lex<'_>, Ctx<'_>, ()>::new(NumParser, neutral, is_num);
      assert_eq!(
        p.parse_input(&mut inp),
        Err(RtErr::Primary),
        "with nowhere to sync to, the original error surfaces"
      );
      assert_eq!(inp.span(), &SimpleSpan::new(0, 0), "no progress committed");
    }

    let holes: usize = input
      .emitter()
      .skipped_regions()
      .values()
      .map(|g| g.len())
      .sum();
    assert_eq!(holes, 0, "a failed sync reports no hole");
    let total: usize = input.emitter().errors().values().map(|g| g.len()).sum();
    assert_eq!(total, 0, "the failed recovery leaves no diagnostics");
  }

  #[test]
  fn skip_then_retry_advances_past_a_sync_point_that_failed_to_parse() {
    //   a ; 1     (sync set: `;` or a number)
    //   0 2 4
    // Cycle 1 skips `a` (one hole) and retries at `;`, which is not a number — a failed
    // retry after real progress, so the stale sync point is consumed and cycle 2 syncs
    // onward, retrying successfully at `1`. Pins the strictly-past-the-sync-point rule.
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::with_state_and_context(
      "a ; 1",
      (),
      crate::input::InputContext::new(
        Verbose::<RtErr>::new(),
        DefaultCache::<'_, Lex<'_>>::default(),
      ),
    );
    {
      let mut inp = input.as_ref();

      let mut p = SkipThenRetry::<_, _, _, (), Lex<'_>, Ctx<'_>, ()>::new(
        NumParser,
        neutral,
        |t: Spanned<&RtTok, &SimpleSpan>| matches!(t.data(), RtTok::Semi | RtTok::Num),
      );
      assert_eq!(p.parse_input(&mut inp), Ok(()), "cycle 2 parses the number");
      assert_eq!(inp.span(), &SimpleSpan::new(4, 5), "the number is consumed");
    }

    assert_eq!(
      input
        .emitter()
        .skipped_regions()
        .get(&SimpleSpan::new(0, 1)),
      Some(&vec![1usize]),
      "cycle 1's hole covers the skipped `a`"
    );
    let holes: usize = input
      .emitter()
      .skipped_regions()
      .values()
      .map(|g| g.len())
      .sum();
    assert_eq!(holes, 1, "cycle 2's zero-skip sync adds no hole");
  }

  #[test]
  fn skip_then_retry_rollback_unwinds_the_hole_emission() {
    // A skipped-then-successful recovery inside an enclosing attempt that declines: the
    // rollback unwinds the hole emission with the log, and a clean re-run records it
    // exactly once again.
    //   a 1
    //   0 2
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::with_state_and_context(
      "a 1",
      (),
      crate::input::InputContext::new(
        Verbose::<RtErr>::new(),
        DefaultCache::<'_, Lex<'_>>::default(),
      ),
    );
    {
      let mut inp = input.as_ref();

      let mut p =
        SkipThenRetry::<_, _, _, (), Lex<'_>, Ctx<'_>, ()>::new(NumParser, neutral, is_num);

      let declined: Option<()> = inp.attempt(|inp| {
        assert_eq!(p.parse_input(inp), Ok(()), "the recovery succeeds inside");
        None
      });
      assert!(declined.is_none(), "the enclosing attempt declines");

      assert_eq!(
        inp.span(),
        &SimpleSpan::new(0, 0),
        "the rollback restores the pre-recovery position"
      );

      // The rolled-back hole emission is gone; re-running records it exactly once.
      assert_eq!(p.parse_input(&mut inp), Ok(()), "the re-run succeeds");
    }

    assert_eq!(
      input
        .emitter()
        .skipped_regions()
        .get(&SimpleSpan::new(0, 1)),
      Some(&vec![1usize]),
      "exactly one hole record survives: the rolled-back one was unwound"
    );
    let holes: usize = input
      .emitter()
      .skipped_regions()
      .values()
      .map(|g| g.len())
      .sum();
    assert_eq!(holes, 1);
  }
}
