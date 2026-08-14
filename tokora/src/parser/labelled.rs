//! The [`labelled`] combinator — attach a *"while parsing X"* diagnostic context to a sub-parse.
//!
//! Wrapping a parser in [`labelled`] pushes a `&'static str` onto the emitter's open-label
//! stack for the duration of the sub-parse and pops it afterwards, so every diagnostic the
//! sub-parse records is stamped with the enclosing context (see [`Verbose`](crate::emitter::Verbose)).
//! Labels are **captured into the emission log at emit time**: each recorded diagnostic carries a
//! snapshot of the labels open when it was emitted, so an emitter rewind that drops an entry drops
//! its labels with it, and a later re-emission re-derives its labels from the then-current stack.
//!
//! The stack lives on the emitter — the one party present at every emit site, including
//! parser-level emissions that have no input access — so a [`labelled`] scope needs nothing beyond
//! the push/pop pair. Around a non-collecting emitter ([`Fatal`](crate::emitter::Fatal),
//! [`Silent`](crate::emitter::Silent)) both calls are inlined-away no-ops, so `labelled(name, p)`
//! reduces to exactly `p`.

use crate::{
  Emitter, InputRef, Lexer, ParseContext, ParseInput, TryParseInput, input::Completeness,
  try_parse_input::ParseAttempt,
};

/// Wraps `parser` with the diagnostic context `name`: for the duration of the sub-parse, `name`
/// is pushed onto the emitter's open-label stack (a *"while parsing X"* context), and every
/// diagnostic recorded during the sub-parse is stamped with the labels open at emit time.
///
/// `name` is `&'static str` — parser names are static, so opening a label never allocates. The
/// label is popped on **every** exit of the sub-parse — success, error, and panic unwind alike —
/// because the pop lives in a drop guard that both paths run, so the live stack always mirrors
/// the nesting of `labelled` scopes even for a host that catches. (Before that guard existed, an
/// unwind skipped the pop and every later diagnostic on that emitter carried the stale context
/// for the emitter's whole life: the label stack is plain emitter state and survives
/// `catch_unwind`.)
///
/// With a non-collecting emitter the push/pop pair are no-ops that inline away, so this wrapper is
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " zero-cost there; a collecting emitter such as [`Verbose`](crate::emitter::Verbose) snapshots the"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " zero-cost there; a collecting emitter such as `Verbose` snapshots the"
)]
/// open labels into each diagnostic and exposes them per-diagnostic via
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " [`Verbose::labels`](crate::emitter::Verbose::labels)."
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " `Verbose::labels`."
)]
///
/// ```
/// # #[cfg(all(feature = "logos_0_16", feature = "std"))]
/// # fn demo<P>(inner: P) -> tokora::Labelled<P> {
/// // Diagnostics emitted inside `inner` are stamped "while parsing a list".
/// tokora::labelled("while parsing a list", inner)
/// # }
/// ```
///
/// When the `trace` feature is on, entering a `labelled` scope also fires a single trace leaf event
/// naming the label at the current depth, so the label context shows up in the parse transcript
/// alongside [`traced`](crate::traced) — keeping the two DX systems coherent.
#[inline(always)]
pub fn labelled<P>(name: &'static str, parser: P) -> Labelled<P> {
  Labelled { name, parser }
}

/// The parser wrapper produced by [`labelled`].
///
/// Delegates to the inner parser, bracketing its run with an
/// [`enter_label`](Emitter::enter_label) / [`exit_label`](Emitter::exit_label) pair on the
/// emitter. Implements both [`ParseInput`] and [`TryParseInput`], so it can wrap either kind of
/// parser — including the element of a `repeated`/`separated` driver.
#[derive(Debug, Clone, Copy)]
pub struct Labelled<P> {
  name: &'static str,
  parser: P,
}

/// The RAII half of a [`labelled`] scope: it owns the reborrowed input for the sub-parse and
/// pops the label in its `Drop`, so the **normal path and the unwind path run the same code**
/// and `exit_label` happens exactly once on both. No disarm flag exists, because there is no
/// path on which the pop should be skipped or repeated.
///
/// It is private to this module: the guard is the mechanism, not a surface.
struct LabelScope<'g, 'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  input: &'g mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> Drop for LabelScope<'_, 'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  fn drop(&mut self) {
    self.input.emitter().exit_label();
  }
}

impl<'inp, L, O, Ctx, Lang, P, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl> for Labelled<P>
where
  Lang: ?Sized,
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    input.emitter().enter_label(self.name);
    // The guard goes up BEFORE the trace event: `trace_event!` formats, and a panicking
    // formatter between the push and the guard would reopen the exact window this closes.
    let scope = LabelScope { input };
    trace_event!(scope.input, self.name);
    let res = self.parser.parse_input(scope.input);
    drop(scope);
    res
  }
}

impl<'inp, L, O, Ctx, Lang, P, Cmpl> TryParseInput<'inp, L, O, Ctx, Lang, Cmpl> for Labelled<P>
where
  Lang: ?Sized,
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline]
  fn try_parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    input.emitter().enter_label(self.name);
    let scope = LabelScope { input };
    trace_event!(scope.input, self.name);
    let res = self.parser.try_parse_input(scope.input);
    drop(scope);
    res
  }
}

#[cfg(all(test, feature = "trace", feature = "logos_0_16", feature = "std"))]
mod trace_tests {
  use crate::{
    InputRef, ParseInput, Token, cache::DefaultCache, emitter::Silent,
    error::token::UnexpectedToken, input::Input, lexer::LogosLexer,
  };

  #[derive(Debug, Clone, PartialEq)]
  enum Err {
    Any,
  }
  impl From<()> for Err {
    fn from(_: ()) -> Self {
      Err::Any
    }
  }
  impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for Err {
    fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
      Err::Any
    }
  }

  #[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
  #[logos(crate = crate::logos, skip r"[ \t\r\n]+")]
  enum Tok {
    #[regex(r"[0-9]+")]
    Num,
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  enum Kind {
    Num,
  }
  impl core::fmt::Display for Kind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      write!(f, "number")
    }
  }

  impl Token<'_> for Tok {
    type Kind = Kind;
    type Error = Err;

    const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
    fn kind(&self) -> Kind {
      Kind::Num
    }
    fn is_trivia(&self) -> bool {
      false
    }
  }

  type Lex<'a> = LogosLexer<'a, Tok>;
  type Cx<'a> = (Silent<Err>, DefaultCache<'a, Lex<'a>>);

  fn eat_num<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Cx<'inp>>) -> Result<bool, Err> {
    inp.try_expect(|_| true).map(|tok| tok.is_some())
  }

  // With the `trace` feature on, entering a `labelled` scope fires exactly one trace leaf line
  // naming the label at the current depth — keeping the label DX coherent with `traced`.
  #[test]
  fn labelled_fires_a_single_trace_leaf_naming_the_label() {
    let mut input = Input::<Lex<'_>, Cx<'_>>::with_state_and_context(
      "12",
      (),
      crate::input::InputContext::new(Silent::<Err>::new(), DefaultCache::<'_, Lex<'_>>::default()),
    );
    let mut inp = input.as_ref();

    let mut parser = crate::labelled("while parsing item", eat_num);
    let (res, lines) = crate::trace::capture(|| parser.parse_input(&mut inp));

    assert_eq!(res, Ok(true));
    // The label surfaces as a leaf line (`·`), naming the label.
    assert!(
      lines
        .iter()
        .any(|l| l.contains("\u{b7}") && l.contains("while parsing item")),
      "labelled fires a trace leaf naming the label: {lines:#?}"
    );
  }
}

// The label must be popped on EVERY exit of the scope, unwind included. The error path
// is already bracketed (`res` is produced, then `exit_label` runs); the unguarded path is
// exactly panic unwind, where the pop is skipped and the label stack — plain emitter state
// that survives `catch_unwind` — keeps the stale context for the emitter's whole life.
#[cfg(all(test, feature = "logos_0_16", feature = "std"))]
mod unwind_tests {
  use crate::{
    InputRef, ParseInput, Token,
    cache::DefaultCache,
    emitter::Verbose,
    error::token::UnexpectedToken,
    input::Input,
    lexer::LogosLexer,
    span::{SimpleSpan, Spanned},
  };

  #[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
  enum Err {
    Any,
  }
  impl From<()> for Err {
    fn from(_: ()) -> Self {
      Err::Any
    }
  }
  impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for Err {
    fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
      Err::Any
    }
  }

  #[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
  #[logos(crate = crate::logos, skip r"[ \t\r\n]+")]
  enum Tok {
    #[regex(r"[0-9]+")]
    Num,
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  enum Kind {
    Num,
  }
  impl core::fmt::Display for Kind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("number")
    }
  }

  impl Token<'_> for Tok {
    type Kind = Kind;
    type Error = Err;

    const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
    fn kind(&self) -> Kind {
      Kind::Num
    }
    fn is_trivia(&self) -> bool {
      false
    }
  }

  type Lex<'a> = LogosLexer<'a, Tok>;
  type Cx<'a> = (Verbose<Err>, DefaultCache<'a, Lex<'a>>);

  /// Emits one diagnostic at `span`, so the caller can read back the labels that were open.
  fn mark<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Cx<'inp>>, at: SimpleSpan) {
    let _ = <Verbose<Err> as crate::Emitter<'inp, Lex<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(at, Err::Any),
    );
  }

  fn boom<'inp>(_inp: &mut InputRef<'inp, '_, Lex<'inp>, Cx<'inp>>) -> Result<(), Err> {
    panic!("D26: the inner parser panics inside the labelled scope")
  }

  #[test]
  fn labelled_pops_its_label_when_the_inner_parser_panics() {
    let mut input = Input::<Lex<'_>, Cx<'_>>::with_state_and_context(
      "12 34",
      (),
      crate::input::InputContext::new(
        Verbose::<Err>::new(),
        DefaultCache::<'_, Lex<'_>>::default(),
      ),
    );
    let mut inp = input.as_ref();

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let mut p = crate::labelled("audit-ctx", boom);
      let _ = p.parse_input(&mut inp);
    }));
    assert!(caught.is_err(), "the panic was caught by this host");

    // The host emits ONE diagnostic after the catch, outside every label scope.
    let after = SimpleSpan::new(0usize, 1usize);
    mark(&mut inp, after);

    assert_eq!(
      inp.emitter().labels()[&after],
      std::vec![std::vec::Vec::<&'static str>::new()],
      "a diagnostic emitted after the caught panic carries no label — `labelled` popped its \
       own scope on the unwind edge"
    );
  }

  /// Runs a panicking `labelled("inner", ..)` under a catch, then — still inside the caller's
  /// own `labelled("outer", ..)` scope — emits one diagnostic so the open-label set is readable.
  fn catch_inner_then_mark<'inp>(
    inp: &mut InputRef<'inp, '_, Lex<'inp>, Cx<'inp>>,
  ) -> Result<(), Err> {
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let mut inner = crate::labelled("inner", boom);
      let _ = inner.parse_input(inp);
    }));
    assert!(caught.is_err(), "the inner panic was caught inside `outer`");
    mark(inp, SimpleSpan::new(0usize, 1usize));
    Ok(())
  }

  #[test]
  fn nested_labels_unwind_pops_exactly_the_unwound_scopes() {
    let mut input = Input::<Lex<'_>, Cx<'_>>::with_state_and_context(
      "12 34",
      (),
      crate::input::InputContext::new(
        Verbose::<Err>::new(),
        DefaultCache::<'_, Lex<'_>>::default(),
      ),
    );
    let mut inp = input.as_ref();

    let inside = SimpleSpan::new(0usize, 1usize);
    let mut outer = crate::labelled("outer", catch_inner_then_mark);
    outer.parse_input(&mut inp).expect("outer completes");

    assert_eq!(
      inp.emitter().labels()[&inside],
      std::vec![std::vec!["outer"]],
      "the unwind popped `inner` and only `inner` — the guard pops per scope, not clear-all"
    );
  }
}
