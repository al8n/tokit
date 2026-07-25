use core::marker::PhantomData;

use crate::{
  Branch, Emitter, InputRef, Lexer, ParseChoice, ParseContext, ParseInput, Span, Token,
  cache::PeekedTokenExt,
  error::{UnexpectedEnd, UnexpectedEot, token::UnexpectedToken},
};

mod fused;
pub use fused::*;

/// A deterministic dispatch combinator driven by a **static table** of viable
/// first-token kinds.
///
/// `DispatchOnKind` is the *kind-keyed* sibling of [`PeekThenChoice`](super::PeekThenChoice):
/// instead of a hand-written handler closure, it takes a `&'static` slice of token
/// [`Kind`](Token::Kind)s — one per branch, **in branch order** — and dispatches on the
/// kind of the next token:
///
/// 1. **Peek** the next token (a single, non-consuming lookahead).
/// 2. **Look up** its kind in the table.
/// 3. On a hit, run the branch at that table index (via [`ParseChoice`]).
/// 4. On a miss — a **committed dispatch failure** — return an [`UnexpectedToken`]
///    whose expected set is *the entire table* (`expected one of …`), reported as
///    [`Expected::OneOf`](crate::utils::Expected::OneOf).
/// 5. On end-of-input at the dispatch point, return an [`UnexpectedEot`].
///
/// # Why the expected set is trustworthy
///
/// The viable first-token set is known **statically** — it is exactly the table. Because the
/// lookahead is non-consuming and nothing speculative runs, a dispatch failure is *committed*:
/// the returned error can never be unwound and lost. This is the intended source of
/// `expected one of …` diagnostics — a static dispatch table, never speculative branch merging.
///
/// # Table ↔ branch correspondence
///
/// The table is indexed **by branch**: `table[i]` is the viable first-token kind for branch `i`.
/// For a tuple of `K` parsers (whose [`ParseChoice::Id`] is [`Branch<K-1>`](Branch)) the table
/// should hold exactly `K` kinds. A branch may of course be reached by only one kind; to route
/// several kinds to the same branch, repeat that branch's kind at each relevant position is *not*
/// supported here — use [`PeekThenChoice`](super::PeekThenChoice) for many-to-one dispatch.
///
/// # Error channel
///
/// The dispatch failure travels the **`Err` channel**: the `UnexpectedToken` (carrying the full
/// expected set) is returned as `Err`, not routed through a bespoke emitter method. Both a
/// fail-fast [`Fatal`](crate::emitter::Fatal) emitter and an error-collecting
/// [`Verbose`](crate::emitter::Verbose) emitter therefore observe the identical payload.
///
/// # The fused twin
///
/// [`FusedDispatchOnKind`] is the *lex-once* sibling: instead of peeking the decision token
/// (a token-cache round trip, including a lexer-state clone) and letting the winning branch
/// consume it again, it lexes once and hands the already-consumed head token to the winning
/// arm. Failures are observationally identical to this combinator's; only the hot hit path
/// differs. Pick per dispatch site: sum-type hot loops prefer the fused shape, while
/// branches that are self-contained [`ParseInput`] parsers (reused elsewhere, or wanting
/// the token left on the input) keep this peek shape.
///
/// # Performance: keep token-kind discriminants dense
///
/// Dispatch cost is one peek plus one table lookup, then the branch. For the *kind match*
/// inside branch parsers — and for any hand-written `match tok.kind()` beside a dispatch
/// table — declare the [`Kind`](Token::Kind) enum with **dense discriminants** (the default
/// `0, 1, 2, …`; avoid sparse explicit values): rustc lowers a match over a dense fieldless
/// enum to a real jump table or lookup, while sparse discriminants degrade to compare
/// chains. The same advice applies to [`FusedDispatchOnKind`], which shares this
/// table-order contract.
///
/// # Examples
///
/// ```ignore
/// use tokora::{Branch, ParseChoice, parser::{Any, ParseInput}};
///
/// // Dispatch on the first token: number → B0, `+` → B1, `(` → B2.
/// let mut parser = (
///     Any::new().map(|_| "number"),
///     Any::new().map(|_| "plus"),
///     Any::new().map(|_| "paren"),
/// )
///     .dispatch_on_kind(&[TokenKind::Number, TokenKind::Plus, TokenKind::LParen]);
///
/// // On an unexpected first token the error carries `expected one of:
/// // 'number', '+', '('` — lifted straight from the table.
/// let _ = parser.parse_input(inp);
/// ```
///
/// [`Kind`]: Token::Kind
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DispatchOnKind<P, Kind: 'static, L, Ctx, Lang: ?Sized = ()> {
  parser: P,
  table: &'static [Kind],
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
}

impl<P, Kind: 'static, L, Ctx, Lang: ?Sized> DispatchOnKind<P, Kind, L, Ctx, Lang> {
  /// Creates a new `DispatchOnKind` combinator over a static kind table.
  #[inline(always)]
  pub(crate) const fn of(parser: P, table: &'static [Kind]) -> Self {
    Self {
      parser,
      table,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
    }
  }
}

/// The classified outcome of the dispatch lookahead.
///
/// Computed while the peek borrow of the input is live, then acted on once the
/// borrow is released so the chosen branch (or the failure) can re-borrow the input.
enum Dispatched<S, T> {
  /// End-of-input at the dispatch point.
  Eot,
  /// The peeked kind matched the branch at this table index.
  Hit(usize),
  /// The peeked kind was not in the table — a committed dispatch failure.
  Miss { span: S, found: T },
}

impl<'inp, P, L, O, Ctx, Lang, const N: usize> ParseInput<'inp, L, O, Ctx, Lang>
  for DispatchOnKind<P, <L::Token as Token<'inp>>::Kind, L, Ctx, Lang>
where
  P: ParseChoice<'inp, L, O, Ctx, Lang, Id = Branch<N>>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  <L::Token as Token<'inp>>::Kind: 'static,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>>
    + From<UnexpectedEot<L::Offset, Lang, <L::Token as Token<'inp>>::Kind>>,
  Lang: ?Sized,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // A table longer than the branch count would let a lookup select an
    // out-of-range branch; the `index <= N` guard below keeps that safe, and this
    // assertion surfaces the misuse in debug builds.
    debug_assert!(
      self.table.len() <= N + 1,
      "dispatch table has more entries than branches",
    );

    // Phase 1: classify the next token while the peek borrow is live.
    let dispatched = match inp.peek_one()? {
      None => Dispatched::Eot,
      Some(peeked) => {
        let token = peeked.token();
        let kind = token.kind();
        match self.table.iter().position(|candidate| *candidate == kind) {
          Some(index) if index <= N => Dispatched::Hit(index),
          _ => Dispatched::Miss {
            span: peeked.span().clone(),
            found: token.clone(),
          },
        }
      }
    };

    // Phase 2: act now that the peek borrow is released.
    match dispatched {
      Dispatched::Hit(index) => self.parser.parse_choice(inp, &Branch::from_index(index)),
      // End of input at a committed dispatch point: report the whole table as the expected set,
      // exactly as the `Miss` arm does — the viable first-token set is precisely `self.table`.
      // Built through `UnexpectedEnd` (not the `UnexpectedEot` alias) so the expected-set element
      // type is inferred as the token kind from `self.table`, matching the `From` bound above.
      Dispatched::Eot => {
        // A terminal scanner stop at the dispatch point peeks as `None` too; mark it terminal so an
        // enclosing recovery re-raises it, while a genuine end of input stays recoverable.
        let eot = UnexpectedEnd::eot_expected_one_of(inp.span().end(), self.table);
        if inp.at_latched_boundary() {
          Err(eot.into_terminal().into())
        } else {
          Err(eot.into())
        }
      }
      Dispatched::Miss { span, found } => Err(
        UnexpectedToken::<_, _, _, Lang>::expected_one_of(span, self.table)
          .with_found(found)
          .into(),
      ),
    }
  }
}

#[cfg(test)]
mod tests;
