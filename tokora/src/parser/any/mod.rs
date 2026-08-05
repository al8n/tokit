use crate::{error::UnexpectedEot, located::Located, span::Span, utils::marker::PhantomLocated};

use super::*;

/// A parser that accepts any single token from the input stream.
///
/// This is the most fundamental parser - it consumes one token regardless of its type.
/// It succeeds if a token is available, and fails only on end-of-input or lexer errors.
///
/// `Any` comes in several variants that determine what information is captured:
/// - **Basic**: Returns just the token value
/// - **Spanned**: Returns token with its [`Span`] (position information)
/// - **Sliced**: Returns token with its source text slice
/// - **Located**: Returns token with both span and slice
///
/// # Type Parameters
///
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Basic Token Consumption
///
/// ```ignore
/// use tokora::parser::{any, ParseInput};
///
/// // Accept any token
/// let parser = any::<MyLexer>();
///
/// // Input: Number(42)      → Ok(Number(42))
/// // Input: Identifier("x") → Ok(Identifier("x"))
/// // Input: (end of input)  → Err(UnexpectedEot)
/// ```
///
/// ## With Span Information
///
/// ```ignore
/// // Capture token with its position
/// let parser = any::<MyLexer>().spanned();
///
/// // Returns: Spanned { data: Token, span: Span { start, end } }
/// ```
///
/// ## With Source Text
///
/// ```ignore
/// // Capture token with its source text
/// let parser = any::<MyLexer>().sliced();
///
/// // Input: "foo" → Ok(Sliced { data: Identifier("foo"), slice: "foo" })
/// ```
///
/// ## With Full Location Info
///
/// ```ignore
/// // Capture token, span, and source
/// let parser = any::<MyLexer>().located();
///
/// // Returns: Located { data: Token, span: Span, slice: &str }
/// ```
///
/// ## Filtering Specific Tokens
///
/// ```ignore
/// // Accept any token, then filter for numbers
/// let parser = any::<MyLexer>()
///     .filter(|tok| {
///         if matches!(tok, Token::Number(_)) {
///             Ok(())
///         } else {
///             Err(ExpectedNumberError::new())
///         }
///     });
///
/// // More efficient alternative: use `expect` instead
/// let parser = expect(|tok| matches!(tok, Token::Number(_)));
/// ```
///
/// # Error Handling
///
/// `Any` can fail with:
/// - `UnexpectedEot`: No more tokens available (end of input)
/// - Lexer errors: The lexer produced an error token
///
/// # When to Use
///
/// - **Building blocks**: As the foundation for more complex parsers
/// - **Generic parsing**: When you need to consume any token
/// - **With filtering**: Combined with `.filter()` or `.filter_map()`
/// - **Development**: Quick prototyping before adding specific token checks
///
/// **Prefer `expect`** when you know which token you want - it provides better error messages.
///
/// # Performance
///
/// - **Memory**: Zero-sized type (no runtime overhead)
/// - **Runtime**: O(1) - single token consumption
/// - **Variants**: `.spanned()`, `.sliced()`, `.located()` have minimal overhead
///
/// # See Also
///
/// - [`Expect`] - Parse a specific token (better error messages)
/// - [`Filter`] - Validate after parsing
/// - [`Spanned`], [`Sliced`], [`Located`] - Output wrapper types
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Any<L, Ctx, Lang: ?Sized = ()> {
  _lxr: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
}

impl<L, Ctx> Any<L, Ctx> {
  /// Creates a parser that accepts any token.
  #[must_use]
  #[inline(always)]
  pub const fn new() -> Self {
    Self::of()
  }
}

impl<L, Ctx, Lang> Any<L, Ctx, Lang> {
  /// Creates a parser that accepts any token.
  #[must_use]
  #[inline(always)]
  pub const fn of() -> Self {
    Any {
      _lxr: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Creates a parser that yields any token with its span.
  #[must_use]
  #[inline(always)]
  pub const fn spanned() -> With<Self, PhantomSpan> {
    With::new(Self::of(), PhantomSpan::PHANTOM)
  }

  /// Creates a parser that yields any token with its source.
  #[must_use]
  #[inline(always)]
  pub const fn sliced() -> With<Self, PhantomSliced> {
    With::new(Self::of(), PhantomSliced::PHANTOM)
  }

  /// Creates a parser that yields any token without its source and span.
  #[must_use]
  #[inline(always)]
  pub const fn located() -> With<Self, PhantomLocated> {
    With::new(Self::of(), PhantomLocated::PHANTOM)
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> ParseInput<'inp, L, L::Token, Ctx, Lang, Cmpl>
  for Any<L, Ctx, Lang>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<UnexpectedEot<L::Offset, Lang>> + From<<L::Token as Token<'inp>>::Error>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<L::Token, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx: ParseContext<'inp, L, Lang>,
  {
    match inp.next_or_stop()? {
      Some(Spanned { data: tok, .. }) => Ok(tok),
      None => Err(UnexpectedEot::eot_of(inp.span().end()).into()),
    }
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl>
  ParseInput<'inp, L, Spanned<L::Token, L::Span>, Ctx, Lang, Cmpl>
  for With<Any<L, Ctx, Lang>, PhantomSpan>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<UnexpectedEot<L::Offset, Lang>> + From<<L::Token as Token<'inp>>::Error>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx: ParseContext<'inp, L, Lang>,
  {
    match inp.next_or_stop()? {
      Some(Spanned { data: tok, span }) => Ok(Spanned::new(span, tok)),
      None => Err(UnexpectedEot::eot_of(inp.span().end()).into()),
    }
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl>
  ParseInput<
    'inp,
    L,
    Sliced<L::Token, <L::Source as Source<L::Offset>>::Slice<'inp>>,
    Ctx,
    Lang,
    Cmpl,
  > for With<Any<L, Ctx, Lang>, PhantomSliced>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<UnexpectedEot<L::Offset, Lang>> + From<<L::Token as Token<'inp>>::Error>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    Sliced<L::Token, <L::Source as Source<L::Offset>>::Slice<'inp>>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    Ctx: ParseContext<'inp, L, Lang>,
  {
    match inp.next_or_stop()? {
      Some(Spanned { data: tok, .. }) => Ok(Sliced::new(inp.slice(), tok)),
      None => Err(UnexpectedEot::eot_of(inp.span().end()).into()),
    }
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl>
  ParseInput<
    'inp,
    L,
    Located<L::Token, L::Span, <L::Source as Source<L::Offset>>::Slice<'inp>>,
    Ctx,
    Lang,
    Cmpl,
  > for With<Any<L, Ctx, Lang>, PhantomLocated>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<UnexpectedEot<L::Offset, Lang>> + From<<L::Token as Token<'inp>>::Error>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    Located<L::Token, L::Span, <L::Source as Source<L::Offset>>::Slice<'inp>>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    Ctx: ParseContext<'inp, L, Lang>,
  {
    match inp.next_or_stop()? {
      Some(Spanned { data: tok, span }) => Ok(Located::new(inp.slice(), span, tok)),
      None => Err(UnexpectedEot::eot_of(inp.span().end()).into()),
    }
  }
}

// Both cells run `Any::spanned().map(..)`, so they need the `map` family too.
#[cfg(all(test, feature = "map"))]
mod tests;

#[cfg(all(
  test,
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
mod slice_tests;
