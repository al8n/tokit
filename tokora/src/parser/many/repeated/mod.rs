use core::marker::PhantomData;

use crate::{
  TryParseInput,
  emitter::FullContainerEmitter,
  try_parse_input::{Accept, Decline},
};

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
/// Unlike [`SeparatedWhile`] which expects delimiters between elements, `Repeated` parses
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
/// use tokora::parser::{ParseInput, Repeated, Action};
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
/// | Feature | `Repeated` | `SeparatedWhile` |
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
/// - [`delimited`](Repeated::delimited) - Wrap in delimiters
/// - [`Collect`](crate::parser::Collect) - Wrapper for collecting elements into a container
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Repeated<F, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  pub(super) f: F,
  _m: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Repeated<F, O, L, Ctx, Lang, Cmpl> {
  /// Creates a new `Repeated` parser.
  #[inline(always)]
  pub(crate) const fn new(f: F) -> Self {
    Self {
      f,
      _m: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Repeated<F, O, L, Ctx, Lang, Cmpl> {
  define_many_delimited_methods!(Lang);
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Repeated<F, O, L, Ctx, Lang, Cmpl> {
  /// Sets the minimum number of elements to parse.
  #[inline(always)]
  pub fn at_least(self, n: usize) -> AtLeast<Repeated<F, O, L, Ctx, Lang, Cmpl>> {
    self.apply(Minimum::new(n))
  }

  /// Sets the maximum number of elements to parse.
  #[inline(always)]
  pub fn at_most(self, n: usize) -> AtMost<Repeated<F, O, L, Ctx, Lang, Cmpl>> {
    self.apply(Maximum::new(n))
  }

  /// Sets both the minimum and maximum number of elements to parse.
  #[inline(always)]
  pub fn bounded(self, min: usize, max: usize) -> Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>> {
    self.apply(With::new(Maximum::new(max), Minimum::new(min)))
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Apply<AtLeast<Self>> for Repeated<F, O, L, Ctx, Lang, Cmpl> {
  type Options = Minimum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> AtLeast<Self> {
    AtLeast::new(self, options.get())
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Apply<AtMost<Self>> for Repeated<F, O, L, Ctx, Lang, Cmpl> {
  type Options = Maximum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> AtMost<Self> {
    AtMost::new(self, options.get())
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Apply<Bounded<Self>> for Repeated<F, O, L, Ctx, Lang, Cmpl> {
  type Options = With<Maximum, Minimum>;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> Bounded<Self> {
    Bounded::new(self, options.primary.get(), options.secondary.get())
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Apply<Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>>>
  for AtMost<Repeated<F, O, L, Ctx, Lang, Cmpl>>
{
  type Options = Minimum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>> {
    Bounded::new(self.parser, self.maximum.get(), options.get())
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Apply<Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>>>
  for AtLeast<Repeated<F, O, L, Ctx, Lang, Cmpl>>
{
  type Options = Maximum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>> {
    Bounded::new(self.parser, options.get(), self.minimum.get())
  }
}

impl<'inp, 'c, L, F, O, Ctx, Lang: ?Sized, Cmpl> Repeated<F, O, L, Ctx, Lang, Cmpl> {
  pub(super) fn parse<Container, RH>(
    &mut self,
    inp: &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
    container: &mut Container,
    rh: &RH,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Ctx::Emitter: Emitter<'inp, L, Lang> + FullContainerEmitter<'inp, L, Lang>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    // The absence-exit gate surfaces a terminal scanner stop as this end-of-input error.
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Container: crate::container::Container<O>,
    RH: RepeatedHandler<'inp, 'c, O, L, Ctx, Lang, Cmpl>,
  {
    trace_event!(inp, "repeated");
    let mut num = 0;
    let mut full = None;
    let anchor = inp.cursor().clone();
    let mut cursor = anchor.clone();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence exit below: comparing the live latch against it
    // keeps that witness attempt-relative. One offset clone per collection, off the per-element path.
    let latch = inp.latch_snapshot();
    // The scanner-trip baseline for the gates below — PER COLLECTION, taken beside the latch
    // and deliberately unlike the per-element descent one. It answers the latch's question
    // through a monotone session counter that no rollback reaches, which is what an element
    // catching a stop inside an `attempt` of its own leaves behind. See
    // `many::absence_after_element` for why the two granularities differ.
    let scans = inp.scanner_trip_snapshot();

    // The trip baseline of the LAST element attempt, carried out by whichever break concluded
    // absence — see `many::absence_after_element` for what the gate below does with it, and why the
    // value has to be carried rather than re-read after the loop.
    let elem_trips = loop {
      // The descent witness's baseline, taken once per ELEMENT — the attempt the chokepoint below
      // judges, and the one the absence gate after the loop judges too.
      // `many::file_element_failure` says why it belongs here and not out beside `latch`.
      let trips = inp.trip_snapshot();
      match self.f.try_parse_input(inp) {
        Ok(Accept(item)) => {
          rh.on_element(num, inp, &anchor)?;
          push_element(&mut num, &mut full, container, item, inp, &anchor)?;
        }
        Ok(Decline) => break trips,
        // File the failure as a diagnostic and keep looping — unless it is one of the three the
        // never-recoverable law forbids spending, in which case re-raise it untouched. The gate is
        // the chokepoint's, not this loop's: see `file_element_failure` for the three witnesses and
        // for why `trips` is taken per ELEMENT rather than per collection. `?` here propagates both
        // a re-raise and an emitter that refused the diagnostic, exactly as the hand-written arms
        // this replaced did.
        Err(err) => file_element_failure(inp, err, &cursor, scans, trips)?,
      }

      // A cycle that consumed nothing re-sees the same input and would retry forever. The progress
      // metric is committed consumption (`span().end()`), never the cache-front cursor — a lookahead
      // fill (a `try_expect` decline pushing a token back) moves that across skipped trivia without
      // consuming, reading a zero-width element as false progress. `<=`, not `==`: the watermark
      // cannot regress within a cycle, so anything not strictly ahead is a stall. `cursor` stays the
      // error-span anchor.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        break trips;
      }
      committed = new_committed;
      cursor = inp.cursor().clone();
    };

    // Both ways out of the loop above — the element declining, and a cycle that committed nothing —
    // conclude *absence*: "no more elements", on the strength of what the last element attempt did.
    // The chokepoint above never sees either, because neither produced an `Err`, so the same two
    // never-recoverable facts have to be witnessed here. `absence_after_element` holds both and says
    // why each baseline is the granularity it is. The end-of-input anchors on the committed end,
    // matching the decision-window and consume gates.
    absence_after_element(inp, &latch, scans, elem_trips)?;

    let span = rh.on_stop(num, inp, &anchor)?;
    // The destination's capacity report goes last, after the count bounds this construct is
    // judged on — see `many::report_full_container`.
    report_full_container(&mut full, inp)?;
    Ok(span)
  }
}
