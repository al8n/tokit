use crate::{TryParseInput, span::Span as _, try_parse_input::ParseAttempt};

use super::*;

/// A combinator that peeks ahead before applying a parser, enabling conditional parsing.
///
/// This combinator looks ahead at the input using a fixed window size, then:
/// 1. **Calls a handler** with the peeked tokens
/// 2. **If handler returns `Ok(())`**: applies the inner parser
/// 3. **If handler returns `Err(e)`**: stops without parsing and returns the error
///
/// Unlike [`PeekThenChoice`] which chooses between multiple parsers, `PeekThen` makes a
/// **binary decision**: parse or don't parse. It's useful for:
/// - **Validation**: Check conditions before committing to parsing
/// - **Early rejection**: Fail fast if lookahead shows incompatible input
/// - **Contextual parsing**: Parse only when specific tokens are present
///
/// # Type Parameters
///
/// - `P`: The inner parser to apply if the condition passes
/// - `D`: Handler function that inspects lookahead and returns `Ok(())` or `Err(...)`
/// - `T`: Token type from the lexer
/// - `Window`: Lookahead window size (e.g., `typenum::U1`, `U2`, etc.)
///
/// # Examples
///
/// ## Basic Conditional Parsing
///
/// ```ignore
/// use tokora::parser::{ParseInput, Action};
/// use generic_arraydeque::typenum::U1;
///
/// // Only parse identifier if it doesn't start with underscore
/// let parser = identifier_parser()
///     .peek_then::<_, U1>(|mut peeked, _emitter| {
///         match peeked.front() {
///             Some(Token::Identifier(name)) if !name.starts_with('_') => Ok(()),
///             _ => Err(InvalidIdentifierError::new()),
///         }
///     });
/// ```
///
/// ## Multi-Token Validation
///
/// ```ignore
/// use generic_arraydeque::typenum::U2;
///
/// // Parse function only if next two tokens are "fn" and an identifier
/// let parser = function_parser()
///     .peek_then::<_, U2>(|mut peeked, _| {
///         let tok1 = peeked.get(0);
///         let tok2 = peeked.get(1);
///
///         match (tok1, tok2) {
///             (Some(Token::Fn), Some(Token::Identifier(_))) => Ok(()),
///             _ => Err(ExpectedFunctionError::new()),
///         }
///     });
/// ```
///
/// ## Context-Aware Parsing
///
/// ```ignore
/// // In a context where we only want numbers
/// let parser = value_parser()
///     .peek_then::<_, U1>(|mut peeked, _| {
///         match peeked.front() {
///             Some(Token::Number(_)) => Ok(()),
///             Some(other) => Err(UnexpectedToken::new(other.kind())),
///             None => Err(UnexpectedEot::new()),
///         }
///     });
/// ```
///
/// # Difference from `PeekThenChoice`
///
/// | Feature | `PeekThen` | `PeekThenChoice` |
/// |---------|-----------|------------------|
/// | **Decision** | Binary (parse or error) | N-way (which parser to use) |
/// | **Input Parser** | Single parser | Tuple of parsers |
/// | **Handler Returns** | `Result<(), E>` | `Result<Id, E>` |
/// | **Use Case** | Validation, filtering | Alternative parsers |
///
/// # Performance
///
/// - **Lookahead cost**: O(W) where W is the window size
/// - **No backtracking**: Parser runs at most once
/// - **Stack allocation**: Lookahead window lives on the stack
///
/// # See Also
///
/// - [`PeekThenChoice`] - Choose between multiple parsers based on lookahead
/// - [`filter`](crate::parser::Filter) - Validate after parsing (no lookahead)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PeekThen<P, D, T, Window, Cmpl = Complete> {
  parser: P,
  handler: D,
  _token: PhantomData<T>,
  _capacity: PhantomData<Window>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, D, T, W: Window, Cmpl> PeekThen<P, D, T, W, Cmpl> {
  /// Creates a new `PeekThen` combinator for the specified language.
  #[inline(always)]
  pub(crate) const fn of<'inp, L, O, Ctx, Lang>(parser: P, condition: D) -> Self
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Lang: ?Sized,
  {
    Self {
      parser,
      handler: condition,
      _token: PhantomData,
      _capacity: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

// STAYS COMPLETE-ONLY (0.3.0 — the decision-window class): the `Decision` peeks a
// scrutinee window (`W = 1` and up), and at a non-final Partial frontier the peek fill silently serves a SHORT
// window (the peek contract: short at the frontier, never an error). The condition would
// read that truncation as "construct ended" and return `Ok` early — breaking chunked
// equivalence with no error on any channel. Generalizing needs the deferred
// frontier-window rule (full-or-incomplete decision windows); until then the impls stay
// pinned at `Complete` in both positions, so a Partial drive is a compile-time wall.
impl<'inp, P, D, L, O, Ctx, Lang, W> ParseInput<'inp, L, O, Ctx, Lang>
  for PeekThen<P, D, L::Token, W>
where
  P: ParseInput<'inp, L, O, Ctx, Lang>,
  D: FnMut(
    Peeked<'_, 'inp, L, W>,
    &mut Ctx::Emitter,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  W: Window,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<crate::error::UnexpectedEot<L::Offset, Lang>>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // A scrutinee window truncated by a terminal scanner stop is not evidence the construct is
    // absent: surface the committed end-of-input error before the handler can route the short
    // window to a branch that succeeds without consuming.
    let end = inp.span().end();
    let (output, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;
    if terminal {
      return Err(
        crate::error::UnexpectedEot::eot_of(end)
          .into_terminal()
          .into(),
      );
    }
    (self.handler)(output, emitter).and_then(|_| self.parser.parse_input(inp))
  }
}

impl<'inp, P, D, L, O, Ctx, Lang, W> TryParseInput<'inp, L, O, Ctx, Lang>
  for PeekThen<P, D, L::Token, W>
where
  P: ParseInput<'inp, L, O, Ctx, Lang>,
  D: Decision<'inp, L, Ctx::Emitter, W, Lang>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  W: Window,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<crate::error::UnexpectedEot<L::Offset, Lang>>,
{
  fn try_parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // A short scrutinee window may be a genuine end of input (decline), but a window truncated
    // by a terminal scanner stop is not: surface the committed end-of-input error rather than
    // reading the stop as "the construct is absent".
    let end = inp.span().end();
    let (output, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;

    if terminal {
      return Err(
        crate::error::UnexpectedEot::eot_of(end)
          .into_terminal()
          .into(),
      );
    }

    if output.is_empty() {
      return Ok(ParseAttempt::Decline);
    }

    self
      .handler
      .decide(output, emitter)
      .and_then(|val| match val {
        Action::Continue => self.parser.parse_input(inp).map(ParseAttempt::Accept),
        Action::Stop => Ok(ParseAttempt::Decline),
      })
  }
}
