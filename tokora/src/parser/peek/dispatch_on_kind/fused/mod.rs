use core::marker::PhantomData;

use crate::{
  Branch, Emitter, InputRef, Lexer, ParseContext, ParseInput, ParseTokenChoice, Span, Token,
  TryParseInput,
  error::{UnexpectedEnd, UnexpectedEot, token::UnexpectedToken},
  try_parse_input::ParseAttempt,
};

/// The **fused** sibling of [`DispatchOnKind`](super::DispatchOnKind): table-driven
/// dispatch that lexes the decision token **once** and hands it to the winning branch,
/// instead of peeking it (a cache round trip) and letting the branch consume it again.
///
/// # The two dispatch shapes
///
/// [`DispatchOnKind`](super::DispatchOnKind) pays the *peek shape*: the lookahead stages
/// a [`CachedToken`](crate::cache::CachedToken) — including a clone of the lexer state —
/// in the token cache, and the winning branch's first consume immediately unstages it.
/// `FusedDispatchOnKind` pays the *fused shape* (the [`try_expect`](InputRef::try_expect)
/// protocol): one scan lexes the head token, classifies its kind against the table, and
/// on a hit **commits it in place** — the token, already lexed and already consumed, is
/// handed to the branch as its `head` argument. Nothing is staged, cloned, or unstaged
/// on the hit path, which is why sum-type hot loops (the "match on token kind" spine of
/// an LALR-style assembly) prefer this shape.
///
/// The trade is the arm contract: branches are [`ParseTokenChoice`] arms
/// (`FnMut(head, inp) -> Result<O, E>`) that receive the consumed head token, rather
/// than self-contained [`ParseInput`] parsers that re-read it. Leaf parsers keep their
/// self-contained shape — this is an assembly-level tool; pick per dispatch site.
///
/// # Observational equivalence on failure
///
/// A failed dispatch consumes nothing and reports exactly what the peek shape reports:
///
/// - **Miss** (the next token's kind is absent from the table): the token is **put back
///   into the cache** — where the peek shape's lookahead would have left it — so the
///   stream state the next parser observes is identical, and the returned
///   [`UnexpectedToken`] carries the *whole* table as its expected set
///   (`expected one of …`), built from the token in hand exactly as
///   [`DispatchOnKind`](super::DispatchOnKind) builds it.
/// - **End of input** (or a latched limit boundary): the same [`UnexpectedEot`] carrying
///   the full table.
/// - **Lexer errors** on the way to the decision token are emitted through the same
///   deduplicated path both shapes share, so the emission log is identical.
///
/// The put-back travels [`try_expect`](InputRef::try_expect)'s existing miss path, so
/// every cache invariant holds unchanged: the staged entry pairs the token with the
/// lexer state observed right after it was lexed (the resume-pairing contract), it is
/// appended through the lineage-counted push (checkpoint restores drop it exactly like
/// a peeked token), and span ordering is untouched because the scan position never
/// advanced past it. A full cache simply drops the put-back — the cursor did not move,
/// so the next operation re-lexes the same token, exactly as an overflowed peek would.
///
/// # Committed and tentative dispatch
///
/// `FusedDispatchOnKind` implements both [`ParseInput`] and [`TryParseInput`]. Calling
/// [`ParseInput::parse_input`] retains the committed behavior above: a table miss returns
/// [`UnexpectedToken`] and end-of-input returns [`UnexpectedEot`]. Calling
/// [`TryParseInput::try_parse_input`] instead returns
/// [`ParseAttempt::Decline`] for a table miss or end-of-input, leaving all valid tokens
/// unconsumed. A hit still commits the head and returns [`ParseAttempt::Accept`]; an error
/// from the selected branch remains an `Err`.
///
/// The committed route uses [`try_expect_map`](InputRef::try_expect_map); the tentative route uses
/// its terminal-aware twin [`try_expect_map_or_stop`](InputRef::try_expect_map_or_stop). Both keep
/// lexer errors' existing emission behavior even when the dispatch ultimately declines. The
/// tentative route additionally tells a real non-table token (a decline) apart from a terminal
/// scanner stop — a resource-limit trip, or the poison boundary it latches — **attempt-relatively**
/// (against the token in hand, never the lex offset), surfacing the terminal stop as an `Err` so a
/// choice caller cannot commit a different alternative over a tripped limit.
///
/// # Why this combinator can never meet a partial-input frontier token
///
/// The cache-withholding rule for [`Partial`](crate::input::Partial) inputs (peek never
/// caches a token touching the buffer end) is preserved trivially: [`ParseInput`] is not
/// completeness-generic — it parses `InputRef<…, Complete>` only — so this combinator
/// runs exclusively on [`Complete`](crate::input::Complete) inputs, where the frontier
/// rules are inert and no frontier token exists to leak. (Even structurally, the scan it
/// uses surfaces `Incomplete` *before* yielding a frontier token, so a put-back of one
/// is unreachable.)
///
/// # Performance: keep token-kind discriminants dense
///
/// Dispatch cost is one lex plus one table lookup, then the branch. For the *kind
/// match* inside your arms — and for any hand-written `match tok.kind()` jump table —
/// declare the [`Kind`](Token::Kind) enum with **dense discriminants** (the default
/// `0, 1, 2, …`; avoid sparse explicit values): rustc lowers a match over a dense
/// fieldless enum to a real jump table or lookup, while sparse discriminants degrade to
/// compare chains. See [`DispatchOnKind`](super::DispatchOnKind)'s notes for the
/// table-order contract shared by both shapes.
///
/// # Examples
///
/// ```ignore
/// use tokora::{Branch, ParseTokenChoice, parser::ParseInput};
///
/// // Fused dispatch on the first token: number → B0, `+` → B1, `(` → B2.
/// // Each arm RECEIVES the consumed head token and parses only the rest.
/// let mut parser = (
///     |head, _inp: &mut _| Ok(Expr::Num(head.data)),
///     |head, inp: &mut _| parse_unary_plus(head, inp),
///     |head, inp: &mut _| parse_parenthesized(head, inp),
/// )
///     .fused_dispatch_on_kind(&[TokenKind::Number, TokenKind::Plus, TokenKind::LParen]);
///
/// // On an unexpected first token the error carries `expected one of:
/// // 'number', '+', '('` — lifted straight from the table — and the token is
/// // put back for whatever runs next.
/// let _ = parser.parse_input(inp);
/// ```
///
/// [`Kind`]: Token::Kind
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FusedDispatchOnKind<P, Kind: 'static, L, Ctx, Lang: ?Sized = ()> {
  parser: P,
  table: &'static [Kind],
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
}

impl<P, Kind: 'static, L, Ctx, Lang: ?Sized> FusedDispatchOnKind<P, Kind, L, Ctx, Lang> {
  /// Creates a new `FusedDispatchOnKind` combinator over a static kind table.
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

impl<'inp, P, L, O, Ctx, Lang, const N: usize> ParseInput<'inp, L, O, Ctx, Lang>
  for FusedDispatchOnKind<P, <L::Token as Token<'inp>>::Kind, L, Ctx, Lang>
where
  P: ParseTokenChoice<'inp, L, O, Ctx, Lang, Id = Branch<N>>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>>
    + From<UnexpectedEot<L::Offset, Lang, <L::Token as Token<'inp>>::Kind>>,
  Lang: ?Sized,
{
  #[inline]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // Same guard as `DispatchOnKind`: a table longer than the branch count would let a
    // lookup select an out-of-range branch; the `index <= N` filter below keeps that
    // safe, and this assertion surfaces the misuse in debug builds.
    debug_assert!(
      self.table.len() <= N + 1,
      "dispatch table has more entries than branches",
    );

    // One fused scan: lex (or serve the cache front), classify by kind, and commit on a
    // hit — `try_expect_map` is the same protocol `try_expect` uses, so the miss path
    // puts the token back through the cache's lineage-counted staging and the hit path
    // adopts span and lexer state exactly like a plain consume. The classifier captures
    // the miss witness (span + token) while the token is in hand, so the diagnostic
    // below never depends on the put-back having found cache room (a zero-capacity
    // cache drops it; the peek shape's overflow serves it without caching — either way
    // the cursor stayed put and the error payload here is identical).
    let table = self.table;
    let mut miss = None;
    let hit = inp.try_expect_map(|tok| {
      match table
        .iter()
        .position(|candidate| *candidate == tok.data.kind())
      {
        Some(index) if index <= N => Some(index),
        _ => {
          miss = Some((tok.span.clone(), tok.data.clone()));
          None
        }
      }
    })?;

    match hit {
      // The winning branch receives the already-lexed head token and parses the rest.
      Some((index, head)) => self
        .parser
        .parse_token_choice(inp, &Branch::from_index(index), head),
      None => match miss {
        // A committed dispatch failure: report the whole table as the expected set,
        // byte-identically to `DispatchOnKind`'s `Miss` arm.
        Some((span, found)) => Err(
          UnexpectedToken::<_, _, _, Lang>::expected_one_of(span, table)
            .with_found(found)
            .into(),
        ),
        // End of input (or a latched limit boundary) at a committed dispatch point: the whole
        // table as the expected set, exactly as `DispatchOnKind`'s `Eot` arm. A terminal scanner
        // stop maps to `None` here too, so mark it terminal — an enclosing recovery re-raises it —
        // while a genuine end of input stays recoverable.
        None => {
          let eot = UnexpectedEnd::eot_expected_one_of(inp.span().end(), table);
          if inp.at_latched_boundary() {
            Err(eot.into_terminal().into())
          } else {
            Err(eot.into())
          }
        }
      },
    }
  }
}

impl<'inp, P, L, O, Ctx, Lang, const N: usize> TryParseInput<'inp, L, O, Ctx, Lang>
  for FusedDispatchOnKind<P, <L::Token as Token<'inp>>::Kind, L, Ctx, Lang>
where
  P: ParseTokenChoice<'inp, L, O, Ctx, Lang, Id = Branch<N>>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  // The tentative route surfaces a terminal stop through `try_expect_map_or_stop`, whose terminal
  // end-of-input error uses the primitive's own (default) expected set — hence the plain
  // `UnexpectedEot<L::Offset, Lang>` bound the whole `_or_stop` family carries, not the
  // table-kinded one the committed route builds.
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
{
  #[inline(always)]
  fn try_parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // As in the committed implementation, never route a table entry to an arm that
    // does not exist. The check preserves the established debug misuse diagnostic.
    debug_assert!(
      self.table.len() <= N + 1,
      "dispatch table has more entries than branches",
    );

    // `try_expect_map_or_stop` commits only hits, and it tells definite absence apart from a
    // terminal scanner stop **attempt-relatively** — against the token in hand, not the lex offset.
    // A cache-hit classifier miss (the next valid token is a real, non-table token, left at the
    // cache front) and a genuine end of input both return `Ok(None)` = a tentative decline; a
    // genuine scan trip, or an already-latched poison boundary at the cursor, instead surfaces the
    // committed forms' terminal end-of-input error on the `Err` channel. This is why a prior wide
    // peek that latched the lex offset ahead of the cursor cannot mis-charge an ordinary miss as
    // terminal (the lex-offset `at_latched_boundary` witness this route used did): a choice caller
    // still takes another alternative on a real non-table token, while a tripped limit is never
    // swallowed as a decline.
    let table = self.table;
    match inp.try_expect_map_or_stop(|tok| {
      table
        .iter()
        .position(|candidate| *candidate == tok.data.kind())
        .filter(|&index| index <= N)
    })? {
      Some((index, head)) => self
        .parser
        .parse_token_choice(inp, &Branch::from_index(index), head)
        .map(ParseAttempt::Accept),
      // Definite absence — a real non-table token at the cache front, or a genuine end of input —
      // is a tentative decline. A terminal scanner stop never reaches here: the primitive raised
      // it on the `Err` channel above.
      None => Ok(ParseAttempt::Decline),
    }
  }
}

#[cfg(test)]
mod tests;
