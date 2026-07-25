use crate::{
  Token,
  error::{MaybeIncomplete, MaybeTerminal},
  input::DelimClass,
};

use super::*;

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
/// # The never-recoverable law
///
/// An [`Incomplete`](crate::error::Incomplete) error is re-raised untouched — before any skip,
/// and from any retry — exactly as [`Recover`] does: recovery synthesizes progress over a
/// *malformed* construct, but an incomplete one is merely unfinished, so skipping would drop
/// input that has not finished arriving. See [`MaybeIncomplete`].
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
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: MaybeIncomplete + MaybeTerminal,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // First attempt, exactly `Recover`'s shape: speculate through `try_attempt` so a failure
    // rolls back to the pre-parse state (position, lexer state, emissions), and re-raise an
    // `Incomplete` — or a terminal scanner stop — untouched before any skip, since no skipping
    // clears either (the never-recoverable law and its terminal dual).
    let mut err = match inp.try_attempt(|input| self.parser.parse_input(input)) {
      Ok(output) => return Ok(output),
      Err(e) if e.is_incomplete() || e.is_terminal() => return Err(e),
      Err(e) => e,
    };

    loop {
      // The cycle's progress anchor: the committed position before this sync.
      let before = inp.cursor().as_inner().clone();

      // Skip to the next depth-0 sync point. The classifier is re-borrowed through a closure
      // (any `DelimClass` is reusable across cycles that way); a fatal emitter rejection
      // mid-skip propagates per the sync family's fatal-exit discipline. A failed sync (no
      // sync point before end of input; it leaves no trace) surfaces this cycle's trigger
      // error — there is nowhere left to retry from.
      let classifier = &mut self.classifier;
      let synced = inp.sync_balanced(
        |kind: &<L::Token as Token<'inp>>::Kind| classifier.classify(kind),
        &mut self.pred,
      )?;
      if synced.is_none() {
        return Err(err);
      }

      match inp.try_attempt(|input| self.parser.parse_input(input)) {
        Ok(output) => return Ok(output),
        // The law applies to every raise: an `Incomplete` or a terminal scanner stop from a retry
        // re-raises unchanged, with no further skipping.
        Err(e) if e.is_incomplete() || e.is_terminal() => return Err(e),
        Err(e) => {
          // The progress guard: a cycle that consumed nothing — zero-skip sync, and the
          // failed retry rolled back to the same spot — must not loop. Bail with the error
          // that triggered this cycle (for the first cycle, the original error).
          if *inp.cursor().as_inner() <= before {
            return Err(err);
          }
          err = e;
          // This sync point did not admit a successful retry: consume it so the next cycle
          // scans strictly past it — the guarantee that every continuing cycle consumes at
          // least one token. Nothing left to consume means nothing left to retry.
          if inp.next()?.is_none() {
            return Err(err);
          }
        }
      }
    }
  }
}

// Recovery behavior needs a lexer that actually runs, which pins the suite to `logos` + `std` —
// the same gate as the `Recover` tests.
#[cfg(all(test, feature = "logos", feature = "std"))]
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
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::new("a b 1");
    let mut emitter = Verbose::<RtErr>::new();
    {
      let mut inp = input.as_ref(&mut emitter);

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
      emitter.skipped_regions().get(&SimpleSpan::new(0, 3)),
      Some(&vec![2usize]),
      "exactly one hole: the skipped `a b`"
    );
    let total: usize = emitter.errors().values().map(|g| g.len()).sum();
    assert_eq!(total, 0, "no per-token diagnostics from the recovery skip");
  }

  #[test]
  fn skip_then_retry_zero_consumption_cycle_bails_with_original_error() {
    // The pinned progress guard: the sync point is immediately at hand (zero-skip) and the
    // retry fails without consuming — the cycle consumed nothing, so the combinator bails
    // out with the ORIGINAL error rather than looping.
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::new("1 2");
    let mut emitter = Verbose::<RtErr>::new();
    let calls = Rc::new(Cell::new(0));
    {
      let mut inp = input.as_ref(&mut emitter);

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
    let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
    assert_eq!(holes, 0, "a zero-skip sync reports no hole");
    let total: usize = emitter.errors().values().map(|g| g.len()).sum();
    assert_eq!(total, 0, "the bailed-out recovery leaves no diagnostics");
  }

  #[test]
  fn skip_then_retry_reraises_incomplete_without_skipping() {
    // The never-recoverable law: an `Incomplete` passes through untouched — the classifier
    // and the sync predicate are never consulted, nothing is skipped, nothing is emitted.
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::new("1 2 3");
    let mut emitter = Verbose::<RtErr>::new();
    let calls = Rc::new(Cell::new(0));
    let classified = Cell::new(false);
    let synced = Cell::new(false);
    {
      let mut inp = input.as_ref(&mut emitter);

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
    let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
    assert_eq!(holes, 0, "nothing was skipped");
    let total: usize = emitter.errors().values().map(|g| g.len()).sum();
    assert_eq!(total, 0, "nothing was emitted");
  }

  #[test]
  fn skip_then_retry_reraises_a_from_incomplete_built_error() {
    // The value W5's frontier rules surface is built via `From<Incomplete>`. Prove that value is
    // recognized as incomplete and re-raised before any skip — the classifier and sync predicate
    // never run, nothing is emitted.
    let surfaced: RtErr = crate::error::Incomplete::new(4usize).into();
    assert!(surfaced.is_incomplete());

    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::new("1 2 3");
    let mut emitter = Verbose::<RtErr>::new();
    let calls = Rc::new(Cell::new(0));
    let classified = Cell::new(false);
    let synced = Cell::new(false);
    {
      let mut inp = input.as_ref(&mut emitter);
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
    let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
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

    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::new("1 2 3");
    let mut emitter = Verbose::<RtErr>::new();
    let calls = Rc::new(Cell::new(0));
    let classified = Cell::new(false);
    let synced = Cell::new(false);
    {
      let mut inp = input.as_ref(&mut emitter);
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
    let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
    assert_eq!(holes, 0, "nothing was skipped");
  }

  #[test]
  fn skip_then_retry_failed_sync_surfaces_the_error() {
    // No sync point before end of input: the failed sync leaves no trace and the original
    // error surfaces.
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::new("a b");
    let mut emitter = Verbose::<RtErr>::new();
    {
      let mut inp = input.as_ref(&mut emitter);

      let mut p =
        SkipThenRetry::<_, _, _, (), Lex<'_>, Ctx<'_>, ()>::new(NumParser, neutral, is_num);
      assert_eq!(
        p.parse_input(&mut inp),
        Err(RtErr::Primary),
        "with nowhere to sync to, the original error surfaces"
      );
      assert_eq!(inp.span(), &SimpleSpan::new(0, 0), "no progress committed");
    }

    let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
    assert_eq!(holes, 0, "a failed sync reports no hole");
    let total: usize = emitter.errors().values().map(|g| g.len()).sum();
    assert_eq!(total, 0, "the failed recovery leaves no diagnostics");
  }

  #[test]
  fn skip_then_retry_advances_past_a_sync_point_that_failed_to_parse() {
    //   a ; 1     (sync set: `;` or a number)
    //   0 2 4
    // Cycle 1 skips `a` (one hole) and retries at `;`, which is not a number — a failed
    // retry after real progress, so the stale sync point is consumed and cycle 2 syncs
    // onward, retrying successfully at `1`. Pins the strictly-past-the-sync-point rule.
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::new("a ; 1");
    let mut emitter = Verbose::<RtErr>::new();
    {
      let mut inp = input.as_ref(&mut emitter);

      let mut p = SkipThenRetry::<_, _, _, (), Lex<'_>, Ctx<'_>, ()>::new(
        NumParser,
        neutral,
        |t: Spanned<&RtTok, &SimpleSpan>| matches!(t.data(), RtTok::Semi | RtTok::Num),
      );
      assert_eq!(p.parse_input(&mut inp), Ok(()), "cycle 2 parses the number");
      assert_eq!(inp.span(), &SimpleSpan::new(4, 5), "the number is consumed");
    }

    assert_eq!(
      emitter.skipped_regions().get(&SimpleSpan::new(0, 1)),
      Some(&vec![1usize]),
      "cycle 1's hole covers the skipped `a`"
    );
    let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
    assert_eq!(holes, 1, "cycle 2's zero-skip sync adds no hole");
  }

  #[test]
  fn skip_then_retry_rollback_unwinds_the_hole_emission() {
    // A skipped-then-successful recovery inside an enclosing attempt that declines: the
    // rollback unwinds the hole emission with the log, and a clean re-run records it
    // exactly once again.
    //   a 1
    //   0 2
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::new("a 1");
    let mut emitter = Verbose::<RtErr>::new();
    {
      let mut inp = input.as_ref(&mut emitter);

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
      emitter.skipped_regions().get(&SimpleSpan::new(0, 1)),
      Some(&vec![1usize]),
      "exactly one hole record survives: the rolled-back one was unwound"
    );
    let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
    assert_eq!(holes, 1);
  }
}
