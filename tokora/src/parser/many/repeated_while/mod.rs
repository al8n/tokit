use core::marker::PhantomData;

use crate::{emitter::FullContainerEmitter, span::Span as _};

use super::*;

mod at_least;
mod at_most;
mod bounded;
mod unbounded;

/// A parser that repeatedly applies an element parser until a condition signals to stop.
///
/// This combinator repeatedly parses elements **without separators** until the `condition`
/// function returns [`Action::Stop`]. It provides fine-grained control over:
/// - **When to stop**: User-defined lookahead-based decision function
/// - **Repetition bounds**: Minimum and maximum number of elements
/// - **Delimiters**: Can wrap in delimiters like `[...]` or `{...}`
///
/// Unlike [`SeparatedWhile`] which expects delimiters between elements, `RepeatedWhile` parses
/// consecutive elements with no separators.
///
/// # Type Parameters
///
/// - `F`: The element parser
/// - `Condition`: Decision function that determines when to stop parsing (receives lookahead)
/// - `O`: Output type of the element parser
/// - `W`: Lookahead window size for the condition
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Config`: Configuration options (min/max bounds)
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Basic Repetition
///
/// ```ignore
/// use tokora::parser::{ParseInput, RepeatedWhile, Action};
/// use generic_arraydeque::typenum::U1;
///
/// // Parse numbers until we hit a non-number token
/// let parser = number_parser()
///     .repeated(|mut peeked: Peeked<_, _, U1>, _| {
///         match peeked.front() {
///             None => Ok(Action::Stop),
///             Some(Token::Number(_)) => Ok(Action::Continue),
///             _ => Ok(Action::Stop),
///         }
///     })
///     .collect::<Vec<_>>();
///
/// // Input: "123 456 789 abc"
/// // Output: Ok(vec![123, 456, 789])
/// ```
///
/// ## With Bounds
///
/// ```ignore
/// // Parse at least 1, at most 10 elements
/// let parser = element_parser()
///     .repeated(stop_condition)
///     .at_least(Minimum::new(1))
///     .at_most(Maximum::new(10))
///     .collect::<Vec<_>>();
/// ```
///
/// ## Delimited Repetition
///
/// ```ignore
/// // Parse: [element element element]
/// let parser = element_parser()
///     .repeated(stop_condition)
///     .delimited_by(
///         |t| matches!(t, Token::BracketOpen),
///         |t| matches!(t, Token::BracketClose),
///         Delimiter::Bracket
///     )
///     .collect::<Vec<_>>();
///
/// // Input: "[1 2 3 4]"
/// // Output: Ok(vec![1, 2, 3, 4])
/// ```
///
/// ## Stop on Specific Token
///
/// ```ignore
/// use generic_arraydeque::typenum::U1;
///
/// // Parse tokens until we see a semicolon
/// let parser = token_parser()
///     .repeated::<_, U1>(|mut peeked, _| {
///         match peeked.front() {
///             Some(Token::Semicolon) | None => Ok(Action::Stop),
///             _ => Ok(Action::Continue),
///         }
///     })
///     .collect::<Vec<_>>();
/// ```
///
/// # How It Works
///
/// 1. **Parse first element**
/// 2. **Loop**:
///    - Call `condition` with lookahead to check if we should continue
///    - If `Action::Continue`: parse next element
///    - If `Action::Stop`: break
/// 3. **Validate** min/max bounds
/// 4. **Collect** parsed elements into container
///
/// # Difference from `SeparatedWhile`
///
/// | Feature | `RepeatedWhile` | `SeparatedWhile` |
/// |---------|-----------|---------------|
/// | **Separators** | ❌ No separators | ✅ Elements separated by delimiter |
/// | **Use Case** | Consecutive elements | Comma/semicolon-separated lists |
/// | **Example** | `1 2 3 4` | `1, 2, 3, 4` |
///
/// # Error Handling
///
/// The parser emits errors via the traits:
/// - [`TooFewEmitter`](crate::emitter::TooFewEmitter): Too few elements (below minimum)
/// - [`TooManyEmitter`](crate::emitter::TooManyEmitter): Too many elements (above maximum)
///
/// # Performance
///
/// - **Memory**: O(1) for the parser itself (elements collected into container)
/// - **Parsing**: O(n) where n is the number of elements
/// - **Lookahead**: O(W) per iteration where W is the window size
///
/// # See Also
///
/// - [`SeparatedWhile`] - Parse elements with separators (e.g., commas)
/// - [`delimited`](RepeatedWhile::delimited) - Wrap in delimiters
/// - [`Collect`](crate::parser::Collect) - Wrapper for collecting elements into a container
/// # Completeness (0.3.0): Complete-only
///
/// Decision-window class: the `Decision` peeks a
/// `W`-window, and at a non-final Partial frontier the peek fill silently serves a SHORT
/// window (the peek contract: short at the frontier, never an error). The condition would
/// read that truncation as "construct ended" and return `Ok` early — breaking chunked
/// equivalence with no error on any channel. Generalizing needs the deferred
/// frontier-window rule (full-or-incomplete decision windows); until then the impls stay
/// pinned at `Complete` in both positions, so a Partial drive is a compile-time wall.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RepeatedWhile<F, Condition, O, W, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  pub(super) f: F,
  pub(super) condition: Condition,
  _m: PhantomData<O>,
  _cap: PhantomData<W>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  /// Creates a new `RepeatedWhile` parser.
  #[inline(always)]
  pub(crate) const fn new(f: F, condition: Condition) -> Self {
    Self::new_in(f, condition)
  }

  /// Creates a new `RepeatedWhile` parser with the given container.
  #[inline(always)]
  const fn new_in(f: F, condition: Condition) -> Self {
    Self {
      f,
      condition,
      _m: PhantomData,
      _cap: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  define_many_delimited_methods!(Lang);
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  /// Sets the minimum number of elements to parse.
  #[inline(always)]
  pub fn at_least(
    self,
    n: usize,
  ) -> AtLeast<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>> {
    self.apply(Minimum::new(n))
  }

  /// Sets the maximum number of elements to parse.
  #[inline(always)]
  pub fn at_most(self, n: usize) -> AtMost<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>> {
    self.apply(Maximum::new(n))
  }

  /// Sets both the minimum and maximum number of elements to parse.
  #[inline(always)]
  pub fn bounded(
    self,
    min: usize,
    max: usize,
  ) -> Bounded<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>> {
    self.apply(With::new(Maximum::new(max), Minimum::new(min)))
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl> Apply<AtLeast<Self>>
  for RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  type Options = Minimum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> AtLeast<Self> {
    AtLeast::new(self, options.get())
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl> Apply<AtMost<Self>>
  for RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  type Options = Maximum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> AtMost<Self> {
    AtMost::new(self, options.get())
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl> Apply<Bounded<Self>>
  for RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  type Options = With<Maximum, Minimum>;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> Bounded<Self> {
    Bounded::new(self, options.primary.get(), options.secondary.get())
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  Apply<Bounded<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>>>
  for AtMost<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>>
{
  type Options = Minimum;

  #[inline(always)]
  fn apply(
    self,
    options: Self::Options,
  ) -> Bounded<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>> {
    Bounded::new(self.parser, self.maximum.get(), options.get())
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  Apply<Bounded<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>>>
  for AtLeast<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>>
{
  type Options = Maximum;

  #[inline(always)]
  fn apply(
    self,
    options: Self::Options,
  ) -> Bounded<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>> {
    Bounded::new(self.parser, options.get(), self.minimum.get())
  }
}

impl<'inp, 'c, L, F, Condition, O, Ctx, Lang: ?Sized, W>
  RepeatedWhile<F, Condition, O, W, L, Ctx, Lang>
{
  fn parse<Container, RH>(
    &mut self,
    inp: &mut InputRef<'inp, 'c, L, Ctx, Lang>,
    container: &mut Container,
    rh: &RH,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    F: ParseInput<'inp, L, O, Ctx, Lang>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
    Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>,
    Ctx: ParseContext<'inp, L, Lang>,
    // The decision-window gate surfaces a mid-window terminal scanner stop as this end-of-input
    // error.
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Container: crate::container::Container<O>,
    RH: RepeatedHandler<'inp, 'c, O, L, Ctx, Lang>,
  {
    trace_event!(inp, "repeated_while");
    let anchor = inp.cursor().clone();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence exits below: comparing the live latch against it
    // keeps that witness attempt-relative. One offset clone per collection.
    let latch = inp.latch_snapshot();
    let mut nums = 0;
    let mut full = false;

    loop {
      // A short decision window can be a genuine end of input, but one truncated by a terminal
      // scanner stop is not: surface the committed end-of-input error before anything else. `decide`
      // is fallible and emitting, and a zero-width element makes no progress that would re-lex the
      // frontier, so neither may run first — no call may preempt the terminal stop. `end` is the
      // committed watermark, captured before the peek (the fill never advances it).
      let end = inp.span().end();
      let (peeked, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;
      if terminal {
        return Err(UnexpectedEot::eot_of(end).into_terminal().into());
      }

      match self.condition.decide(peeked, emitter)? {
        Action::Stop => {
          // A stop concludes *absence*: "no more elements". The element's own lookahead can latch a
          // terminal scanner stop and still return `Ok` with a short window, leaving the pre-trip
          // tokens cached — this window is then served whole from that cache, so it carries no
          // terminal flag for the gate above to see and the condition reads a truncated view as the
          // end of the construct. Attempt-relative, so an inherited boundary is not mis-charged here.
          if inp.latched_during_attempt(&latch) {
            return Err(
              UnexpectedEot::eot_of(inp.span().end())
                .into_terminal()
                .into(),
            );
          }
          let span = inp.span_since(&anchor);
          return rh.on_stop(nums, inp, &anchor).map(|_| span);
        }
        Action::Continue => {
          // The maximum hook runs only once the element has actually, successfully parsed —
          // matching the try-driven `Repeated::parse`, which never sees `Ok(Accept(item))` (and
          // so never calls `rh.on_element`) for an element that failed. Checking `nums == max`
          // ahead of `parse_input` fired the hook for an element that was merely *about to be
          // attempted*: a `Continue` decision at the boundary paired with a failing parse then
          // reported `TooMany` (or masked the real error under a fail-fast emitter) for an
          // element that was never parsed, contradicting the parsed-element accounting
          // convention `push_element` establishes below.
          let item = self.f.parse_input(inp)?;
          rh.on_element(nums, inp, &anchor)?;
          push_element(&mut nums, &mut full, container, item, inp, &anchor)?;
        }
      }

      // A `Continue` cycle that consumed nothing re-sees the same lookahead and would decide
      // `Continue` forever. The progress metric is committed consumption (`span().end()`), never the
      // cache-front cursor — a lookahead fill moves that across skipped trivia without consuming,
      // reading a zero-width element as false progress. `<=`, not `==`: the watermark cannot regress
      // within a cycle, so anything not strictly ahead is a stall.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        // A stall concludes *absence*: "no more elements". The element's own lookahead can latch a
        // terminal scanner stop and still return `Ok` with a short window, so that conclusion may
        // rest on a truncated view — the decision gate above cannot see it, because the latch happens
        // after it. Surface the stop instead of ending cleanly; attempt-relative against the entry
        // snapshot, so an inherited boundary is not mis-charged here.
        if inp.latched_during_attempt(&latch) {
          return Err(
            UnexpectedEot::eot_of(inp.span().end())
              .into_terminal()
              .into(),
          );
        }
        let span = inp.span_since(&anchor);
        return rh.on_stop(nums, inp, &anchor).map(|_| span);
      }
      committed = new_committed;
    }
  }
}
