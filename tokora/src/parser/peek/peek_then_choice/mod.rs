use crate::{ParseChoice, TryParseInput, span::Span as _, try_parse_input::ParseAttempt};

use super::*;

/// A combinator that chooses between multiple parser alternatives based on lookahead.
///
/// This provides **deterministic choice** by peeking ahead at the input and selecting
/// which parser to use based on the observed tokens. Unlike backtracking-based choice
/// (e.g., `p1.or(p2)`), the decision is made upfront using a fixed lookahead window.
///
/// # How It Works
///
/// 1. **Peek ahead** at up to `W` tokens (where `W` is a compile-time constant like `U1`, `U2`, etc.)
/// 2. **Call the handler** with the peeked tokens and emitter
/// 3. **Handler returns an ID** indicating which parser alternative to use
/// 4. **Execute the selected parser** without backtracking
///
/// # Type Parameters
///
/// - `P`: A tuple of parsers that implements [`ParseChoice`]
/// - `H`: Handler function that inspects lookahead and returns which parser to use
/// - `L`: The lexer type
/// - `Ctx`: Parse context (contains emitter and cache)
/// - `W`: Lookahead window size (e.g., `typenum::U1` for 1 token, `U2` for 2 tokens)
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ```ignore
/// use tokora::{utils::typenum::U2, Branch};
///
/// // Peek at 2 tokens to distinguish `let x` from `let mut x`
/// let parser = (
///     parse_let_binding(),
///     parse_let_mut_binding(),
/// ).peek_then_choice::<_, U2>(|mut peeked, _emitter| {
///     let tok1 = peeked.pop_front();
///     let tok2 = peeked.pop_front();
///
///     match (tok1, tok2) {
///         (Some(Token::Let), Some(Token::Mut)) => Ok(Branch::B1),  // let mut
///         (Some(Token::Let), _) => Ok(Branch::B0),                 // let
///         _ => Err(...),
///     }
/// });
/// ```
///
/// # Determinism vs Backtracking
///
/// **Traditional backtracking** (nom, chumsky, etc.):
/// ```ignore
/// // Tries each parser in sequence, backtracks on failure
/// p1.or(p2).or(p3)  // Can be slow, non-deterministic
/// ```
///
/// **Tokora (deterministic)**:
/// ```ignore
/// // Looks ahead once, makes decision, no backtracking
/// (p1, p2, p3).peek_then_choice::<_, U1>(|peeked, _| {
///     // Return ID based on lookahead
/// })
/// ```
///
/// # Performance
///
/// - **Lookahead cost**: O(W) where W is the window size (typically 1-4)
/// - **No backtracking**: Each alternative is tried at most once
/// - **Stack allocation**: Lookahead window lives on the stack
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PeekThenChoice<P, H, L, Ctx, W, Lang: ?Sized = ()> {
  parser: P,
  handler: H,
  _capacity: PhantomData<W>,
  _ctx: PhantomData<Ctx>,
  _l: PhantomData<L>,
  _lang: PhantomData<Lang>,
}

impl<P, H, L, Ctx, W: Window, Lang: ?Sized> PeekThenChoice<P, H, L, Ctx, W, Lang> {
  /// Creates a new `PeekThenChoice` combinator for the specified language.
  #[inline(always)]
  pub(crate) const fn of<'inp, O, Cmpl>(parser: P, condition: H) -> Self
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    P: ParseChoice<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    Self {
      parser,
      handler: condition,
      _capacity: PhantomData,
      _ctx: PhantomData,
      _l: PhantomData,
      _lang: PhantomData,
    }
  }
}

impl<'inp, P, H, L, O, Ctx, Lang, W: Window> ParseInput<'inp, L, O, Ctx, Lang>
  for PeekThenChoice<P, H, L, Ctx, W, Lang>
where
  P: ParseChoice<'inp, L, O, Ctx, Lang>,
  H: FnMut(
    Peeked<'_, 'inp, L, W>,
    &mut Ctx::Emitter,
  ) -> Result<P::Id, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<crate::error::UnexpectedEot<L::Offset, Lang>>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // A dispatch window truncated by a terminal scanner stop is not a definite branch decision:
    // surface the committed end-of-input error before the handler can route the short window to a
    // branch that succeeds without consuming.
    let end = inp.span().end();
    let id = {
      let (output, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;
      if terminal {
        return Err(
          crate::error::UnexpectedEot::eot_of(end)
            .into_terminal()
            .into(),
        );
      }
      (self.handler)(output, emitter)?
    };
    self.parser.parse_choice(inp, &id)
  }
}

impl<'inp, P, H, L, O, Ctx, Lang, W: Window> TryParseInput<'inp, L, O, Ctx, Lang>
  for PeekThenChoice<P, H, L, Ctx, W, Lang>
where
  P: ParseChoice<'inp, L, O, Ctx, Lang>,
  H: FnMut(
    Peeked<'_, 'inp, L, W>,
    &mut Ctx::Emitter,
  ) -> Result<Option<P::Id>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<crate::error::UnexpectedEot<L::Offset, Lang>>,
{
  #[inline(always)]
  fn try_parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // A dispatch window truncated by a terminal scanner stop is not a definite decline: surface
    // the committed end-of-input error rather than letting the handler read the short window as
    // "no branch applies".
    let end = inp.span().end();
    let id = {
      let (output, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;
      if terminal {
        return Err(
          crate::error::UnexpectedEot::eot_of(end)
            .into_terminal()
            .into(),
        );
      }
      (self.handler)(output, emitter)?
    };
    self.parser.try_parse_choice(inp, id.as_ref())
  }
}

#[cfg(test)]
mod tests;
