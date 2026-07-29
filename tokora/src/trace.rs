//! Parser tracing — the `trace` feature.
//!
//! Wrap any parser in [`traced`] and its execution prints to stderr as an indented event
//! tree: an `enter` line as the parser starts, the crate's own instrumented combinators
//! (`try_expect`, `peek`, the `sync` family, the transaction guards' begin/commit/rollback,
//! `attempt`/`try_attempt`, and the separated/repeated drivers) firing beneath it at the
//! current depth, and an `exit` line carrying the consumed span (`ok`), a `decline`, or an
//! `err`. Each line shows a short preview of the source at the cursor.
//!
//! Tracing is strictly **out of band**: events go to stderr (or, under `cfg(test)`, an
//! internal capture buffer), never through the [`Emitter`](crate::Emitter). A speculative
//! branch that rewinds its emissions therefore never rewinds its trace — the debug output
//! survives the rollback that erased the diagnostics.
//!
//! # Zero-cost when off
//!
//! With the `trace` feature disabled, [`traced`] is compiled as the identity
//! (`traced(name, p)` *is* `p`), the [depth counter](crate::InputRef) field disappears, and
//! every internal hook site — written `trace_event!(input, "name")` — expands to nothing.
//! The instrumentation is not a runtime branch that is skipped; it is absent from the
//! generated code entirely.

/// Internal hook: emit a leaf trace event naming an instrumented combinator, at the input's
/// current depth. Expands to nothing when the `trace` feature is off, so the call site is
/// compiled away rather than guarded at runtime.
#[cfg(feature = "trace")]
macro_rules! trace_event {
  ($input:expr, $name:expr $(,)?) => {{
    $input.trace_leaf($name);
  }};
}

/// Feature-off form: the hook site expands to nothing at all.
#[cfg(not(feature = "trace"))]
macro_rules! trace_event {
  ($($tt:tt)*) => {};
}

/// Wraps `parser` so its run prints an indented enter / exit / rollback event tree to stderr,
/// interleaved with the crate's own instrumented combinators as they fire — the tracing DX
/// that turns an opaque parse into a readable transcript.
///
/// Each `enter` shows a short preview of the source at the cursor; each `exit` reports the
/// outcome — `ok` with the span the parser consumed, `decline` (for a
/// [`TryParseInput`](crate::TryParseInput) that backed out without consuming), or `err`.
/// Nesting one `traced` parser inside another indents the child, so the printed tree mirrors
/// the grammar. Output is out of band (stderr), so a speculative rollback never eats it.
///
/// ```
/// # #[cfg(all(feature = "trace", feature = "logos", feature = "std"))]
/// # fn demo<'inp, P>(inner: P) -> tokora::Traced<P> {
/// // `expr` will print `> expr … / < expr = ok …` around `inner`'s run.
/// tokora::traced("expr", inner)
/// # }
/// ```
///
/// With the `trace` feature off this is the identity — `traced(name, parser)` is exactly
/// `parser`, with no wrapper type and no runtime cost.
#[cfg(feature = "trace")]
#[cfg_attr(docsrs, doc(cfg(feature = "trace")))]
#[inline]
pub fn traced<P>(name: &'static str, parser: P) -> Traced<P> {
  Traced { name, parser }
}

/// Identity when the `trace` feature is off: `traced(name, parser)` is exactly `parser`, so
/// the wrapper and all of its tracing vanish from the build.
#[cfg(not(feature = "trace"))]
#[inline(always)]
pub fn traced<P>(_name: &'static str, parser: P) -> P {
  parser
}

/// The parser wrapper produced by [`traced`] (only when the `trace` feature is on).
///
/// Delegates to the inner parser, bracketing its run with an `enter`/`exit` trace event pair
/// at the input's current depth. Implements both [`ParseInput`](crate::ParseInput) and
/// [`TryParseInput`](crate::TryParseInput), so it can wrap either kind of parser — including
/// the element of a `repeated`/`separated` driver.
#[cfg(feature = "trace")]
#[cfg_attr(docsrs, doc(cfg(feature = "trace")))]
#[derive(Debug, Clone, Copy)]
pub struct Traced<P> {
  name: &'static str,
  parser: P,
}

#[cfg(feature = "trace")]
mod on {
  use super::Traced;
  use crate::{
    Emitter, InputRef, Lexer, ParseContext, ParseInput, TryParseInput, input::Completeness,
    try_parse_input::ParseAttempt,
  };

  /// The disarm half of a [`traced`] scope's depth bracket.
  ///
  /// `trace_enter` bumps the depth and the exit arms drop it again — but only on a *return*
  /// exit, so an unwind through the sub-parse left the depth raised for the input's whole life
  /// and every later trace line indented one level too deep. This guard closes that edge
  /// **without touching the arms**: it is disarmed the moment the sub-parse returns, so the
  /// arms still decrement-then-emit exactly as before (which is what keeps the exit line at the
  /// enter line's indentation), and it fires only when no arm ran.
  struct DepthScope<'g, 'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness,
  {
    input: &'g mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    armed: bool,
  }

  impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> Drop for DepthScope<'_, 'inp, '_, L, Ctx, Lang, Cmpl>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness,
  {
    fn drop(&mut self) {
      if self.armed {
        self.input.trace_unwind_pop();
      }
    }
  }

  impl<'inp, L, O, Ctx, Lang, P, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl> for Traced<P>
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
      let start = input.offset().clone();
      input.trace_enter(self.name);
      let mut scope = DepthScope { input, armed: true };
      let res = self.parser.parse_input(scope.input);
      scope.armed = false;
      match &res {
        Ok(_) => scope.input.trace_exit_ok(self.name, &start),
        Err(_) => scope.input.trace_exit(self.name, "err"),
      }
      res
    }
  }

  impl<'inp, L, O, Ctx, Lang, P, Cmpl> TryParseInput<'inp, L, O, Ctx, Lang, Cmpl> for Traced<P>
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
      let start = input.offset().clone();
      input.trace_enter(self.name);
      let mut scope = DepthScope { input, armed: true };
      let res = self.parser.try_parse_input(scope.input);
      scope.armed = false;
      match &res {
        Ok(ParseAttempt::Accept(_)) => scope.input.trace_exit_ok(self.name, &start),
        Ok(ParseAttempt::Decline) => scope.input.trace_exit(self.name, "decline"),
        Err(_) => scope.input.trace_exit(self.name, "err"),
      }
      res
    }
  }
}

/// Routes a finished trace line out of band: to the `cfg(test)` capture buffer when one is
/// installed, otherwise to stderr. Never touches the emitter.
#[cfg(feature = "trace")]
pub(crate) fn write_line(line: std::string::String) {
  #[cfg(test)]
  {
    let captured = SINK.with_borrow_mut(|slot| match slot.as_mut() {
      Some(buf) => {
        buf.push(line.clone());
        true
      }
      None => false,
    });
    if captured {
      return;
    }
  }
  std::eprintln!("{line}");
}

// Thread-local capture buffer used only by the crate's own tests to assert the trace event
// sequence without racing on the process-wide stderr. Not part of the public surface.
#[cfg(all(feature = "trace", test))]
thread_local! {
  static SINK: core::cell::RefCell<Option<std::vec::Vec<std::string::String>>> =
    const { core::cell::RefCell::new(None) };
}

/// Runs `f` with trace output captured into a buffer instead of stderr, returning `f`'s
/// value alongside the lines it emitted, in order. Test-only, and gated to match its sole
/// consumer — the `logos`/`std`-backed capture test below — so it is not dead code in a
/// `trace`-without-`logos` test build.
#[cfg(all(test, feature = "trace", feature = "logos", feature = "std"))]
pub(crate) fn capture<R>(f: impl FnOnce() -> R) -> (R, std::vec::Vec<std::string::String>) {
  SINK.with_borrow_mut(|slot| *slot = Some(std::vec::Vec::new()));
  let value = f();
  let lines = SINK.with_borrow_mut(|slot| slot.take().unwrap_or_default());
  (value, lines)
}

#[cfg(all(test, feature = "trace", feature = "logos", feature = "std"))]
mod tests {
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

  // The capture test: a one-token parse wrapped in `traced` must print exactly enter, the
  // internal `try_expect` leaf indented one level, then exit-ok with the consumed span.
  #[test]
  fn traced_emits_enter_leaf_exit_with_indentation() {
    let mut emitter = Silent::<Err>::new();
    let mut input = Input::<Lex<'_>, Cx<'_>>::with_state_and_cache(
      "12",
      (),
      DefaultCache::<'_, Lex<'_>>::default(),
    );
    let mut inp = input.as_ref(&mut emitter);

    let mut parser = crate::traced("num", eat_num);
    let (res, lines) = crate::trace::capture(|| parser.parse_input(&mut inp));

    assert_eq!(res, Ok(true));
    assert_eq!(lines.len(), 3, "unexpected event sequence: {lines:#?}");
    // enter at depth 0
    assert!(lines[0].starts_with("> num"), "enter line: {:?}", lines[0]);
    // the internal try_expect hook fires one level deeper (two-space indent)
    assert!(
      lines[1].starts_with("  \u{b7} try_expect"),
      "leaf line: {:?}",
      lines[1]
    );
    // exit-ok back at depth 0, carrying the consumed span
    assert!(lines[2].starts_with("< num"), "exit line: {:?}", lines[2]);
    assert!(
      lines[2].contains("ok"),
      "exit carries ok+span: {:?}",
      lines[2]
    );
    assert!(
      lines[2].contains("0..2"),
      "consumed span 0..2: {:?}",
      lines[2]
    );
  }

  fn boom<'inp>(_inp: &mut InputRef<'inp, '_, Lex<'inp>, Cx<'inp>>) -> Result<bool, Err> {
    panic!("D45/traced: the inner parser panics inside the traced scope")
  }

  // §3.3 — the D26 class, member for member: `trace_enter` bumps the depth, the exit arms
  // decrement it, and an unwind through the sub-parse skips them, so every later trace line
  // is indented one level too deep for the emitter's whole life.
  #[test]
  fn traced_unwind_restores_depth() {
    let mut emitter = Silent::<Err>::new();
    let mut input = Input::<Lex<'_>, Cx<'_>>::with_state_and_cache(
      "12 34",
      (),
      DefaultCache::<'_, Lex<'_>>::default(),
    );
    let mut inp = input.as_ref(&mut emitter);

    let (_, lines) = crate::trace::capture(|| {
      let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut p = crate::traced("boom", boom);
        let _ = p.parse_input(&mut inp);
      }));
      assert!(caught.is_err(), "the panic was caught by this host");
      let mut after = crate::traced("after", eat_num);
      let _ = after.parse_input(&mut inp);
    });

    let enter = lines
      .iter()
      .find(|l| l.contains("> after"))
      .unwrap_or_else(|| panic!("no post-unwind enter line: {lines:#?}"));
    assert!(
      enter.starts_with("> after"),
      "the unwound scope leaked its depth: {enter:?} (all lines: {lines:#?})"
    );
  }
}
