use core::marker::PhantomData;

use crate::{TryParseInput, parse_state::ParseState, try_parse_input::ParseAttempt};

use super::*;

/// A parser that transforms output using an infallible mapping function.
///
/// This combinator applies a **pure transformation** to the successfully parsed value,
/// converting it from type `O` to type `U`. Unlike [`FilterMap`] which can fail,
/// `Map` transformations **always succeed**.
///
/// This is one of the most common combinators, used for:
/// - **Type conversions**: Wrapping values in newtypes or enums
/// - **Extraction**: Pulling out specific fields from parsed structures
/// - **Computation**: Deriving values from parsed data
///
/// # Type Parameters
///
/// - `F`: The inner parser
/// - `G`: Mapping function `FnMut(O) -> U` (infallible)
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `O`: Input type from the inner parser
/// - `O2`: Output type after transformation
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Basic Transformation
///
/// ```ignore
/// use tokora::parser::{any, ParseInput};
///
/// // Parse a token and extract its text
/// let parser = any::<MyLexer>()
///     .sliced()
///     .map(|sliced| sliced.slice.to_string());
///
/// // Input: "hello" → Ok("hello".to_string())
/// ```
///
/// ## Wrapping in Enum
///
/// ```ignore
/// // Parse number and wrap in AST node
/// let parser = parse_number()
///     .map(|n| AstNode::Literal(n));
///
/// // Input: "42" → Ok(AstNode::Literal(42))
/// ```
///
/// ## Field Extraction
///
/// ```ignore
/// // Parse token and extract just the kind
/// let parser = any::<MyLexer>()
///     .map(|tok| tok.kind());
///
/// // Returns TokenKind instead of Token
/// ```
///
/// ## Tuple Construction
///
/// ```ignore
/// // Parse identifier and line number, combine into struct
/// let parser = parse_identifier()
///     .then(parse_line_number())
///     .map(|(name, line)| Definition { name, line });
/// ```
///
/// ## Computation
///
/// ```ignore
/// // Parse two numbers and compute their sum
/// let parser = parse_number()
///     .then_ignore(parse_plus())
///     .then(parse_number())
///     .map(|(a, b)| a + b);
///
/// // Input: "3 + 4" → Ok(7)
/// ```
///
/// ## Chaining Transformations
///
/// ```ignore
/// // Multiple transformations
/// let parser = any::<MyLexer>()
///     .map(|tok| tok.as_string())        // Token -> String
///     .map(|s| s.to_uppercase())         // String -> String
///     .map(|s| Symbol::new(s));          // String -> Symbol
/// ```
///
/// # How It Works
///
/// 1. **Parse**: Inner parser runs and produces `Result<O, E>`
/// 2. **Transform**: If parsing succeeded, mapper function converts `O` to `U`
/// 3. **Return**: Return `Ok(U)` or propagate error
///
/// The mapper is only called on **successful parses** - errors bypass it entirely.
///
/// # Comparison with Related Combinators
///
/// | Combinator | Can Fail | Transforms Type | Use Case |
/// |------------|----------|----------------|----------|
/// | **Map** | ❌ Never | ✅ Yes (`O -> U`) | Infallible transformations |
/// | **FilterMap** | ✅ Yes | ✅ Yes (`O -> Result<U>`) | Fallible transformations |
/// | **Filter** | ✅ Yes | ❌ No (keeps `O`) | Validation only |
/// | **Ignore** | ❌ Never | ✅ Yes (`O -> ()`) | Discard value |
///
/// **When to use**:
/// - `Map`: Transform without possibility of failure (wrapping, extraction, computation)
/// - `FilterMap`: Transform with validation (parsing strings, enum conversion)
/// - `Filter`: Validate without changing type (range checks, pattern matching)
///
/// # Performance
///
/// - **Memory**: O(1) overhead (just the mapper closure, often zero-sized)
/// - **Runtime**: O(1) - single function call after parsing
/// - **Zero-cost**: Typically optimized to inline the mapping function
///
/// # See Also
///
/// - [`FilterMap`] - Fallible transformation (can return errors)
/// - [`Filter`] - Validation without transformation
/// - [`MapWith`] - Transform with access to parse state
/// - [`Ignore`] - Special case: map to `()`
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Map<F, G, L, Ctx, O, O2, Lang: ?Sized = (), Cmpl = Complete> {
  parser: F,
  map_fn: G,
  _o: PhantomData<O>,
  _o2: PhantomData<O2>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<F, G, L, Ctx, O, O2, Lang: ?Sized, Cmpl> Map<F, G, L, Ctx, O, O2, Lang, Cmpl> {
  /// Creates a new `Map` combinator.
  #[inline(always)]
  pub(crate) const fn new(parser: F, map_fn: G) -> Self {
    Self {
      parser,
      map_fn,
      _o: PhantomData,
      _o2: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, F, G, L, O, U, Ctx, Lang, Cmpl> ParseInput<'inp, L, U, Ctx, Lang, Cmpl>
  for Map<F, G, L, Ctx, O, U, Lang, Cmpl>
where
  F: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  G: FnMut(O) -> U,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.parser.parse_input(input).map(&mut self.map_fn)
  }
}

impl<'inp, F, G, L, O, U, Ctx, Lang, Cmpl> TryParseInput<'inp, L, U, Ctx, Lang, Cmpl>
  for Map<F, G, L, Ctx, O, U, Lang, Cmpl>
where
  F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  G: FnMut(O) -> U,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn try_parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<ParseAttempt<U>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self
      .parser
      .try_parse_input(input)
      .map(|res| res.map(&mut self.map_fn))
  }
}

/// A parser that transforms output with access to parse state (context, span, emitter).
///
/// This is the stateful variant of [`Map`] that provides the transformation function
/// with access to [`ParseState`], enabling context-aware transformations based on:
/// - **Parse position**: Current cursor and span information
/// - **Parse context**: User-defined context data
/// - **Emitter**: For complex scenarios (though mapping should rarely emit errors)
///
/// Use this when your transformation needs information beyond the parsed value itself.
///
/// # Type Parameters
///
/// - `F`: The inner parser
/// - `G`: Mapping function `FnMut(O, ParseState) -> U` (infallible)
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `O`: Input type from the inner parser
/// - `O2`: Output type after transformation
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Position-Aware Transformation
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Tag AST nodes with their source location
/// let parser = parse_expression()
///     .map_with(|expr, state| {
///         LocatedExpr {
///             expr,
///             span: state.span(),
///         }
///     });
/// ```
///
/// ## Context-Based Transformation
///
/// ```ignore
/// // Resolve identifiers using parse context
/// let parser = parse_identifier()
///     .map_with(|name, state| {
///         let symbol_id = state.context().intern_symbol(&name);
///         Symbol { name, id: symbol_id }
///     });
/// ```
///
/// ## Combining Span and Value
///
/// ```ignore
/// // Create AST node with location metadata
/// let parser = parse_literal()
///     .map_with(|value, state| {
///         AstNode {
///             kind: NodeKind::Literal(value),
///             span: state.span(),
///             source: state.slice().to_string(),
///         }
///     });
/// ```
///
/// # See Also
///
/// - [`Map`] - Simpler transformation without parse state access
/// - [`FilterMapWith`] - Fallible transformation with parse state
/// - [`ParseState`] - The state object passed to mappers
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MapWith<F, G, L, Ctx, O, O2, Lang: ?Sized = (), Cmpl = Complete> {
  parser: F,
  map_fn: G,
  _o: PhantomData<O>,
  _o2: PhantomData<O2>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<F, G, L, Ctx, O, O2, Lang: ?Sized, Cmpl> MapWith<F, G, L, Ctx, O, O2, Lang, Cmpl> {
  /// Creates a new `Map` combinator.
  #[inline(always)]
  pub(crate) const fn new(parser: F, map_fn: G) -> Self {
    Self {
      parser,
      map_fn,
      _o: PhantomData,
      _o2: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, F, G, L, O, U, Ctx, Lang, Cmpl> ParseInput<'inp, L, U, Ctx, Lang, Cmpl>
  for MapWith<F, G, L, Ctx, O, U, Lang, Cmpl>
where
  F: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  G: FnMut(O, ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>) -> U,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let cursor = input.cursor().clone();
    self
      .parser
      .parse_input(input)
      .map(|output| (self.map_fn)(output, ParseState::new(input, cursor)))
  }
}

impl<'inp, F, G, L, O, U, Ctx, Lang, Cmpl> TryParseInput<'inp, L, U, Ctx, Lang, Cmpl>
  for MapWith<F, G, L, Ctx, O, U, Lang, Cmpl>
where
  F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  G: FnMut(O, ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>) -> U,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn try_parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<ParseAttempt<U>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let cursor = input.cursor().clone();
    self
      .parser
      .try_parse_input(input)
      .map(|res| res.map(|output| (self.map_fn)(output, ParseState::new(input, cursor))))
  }
}

// The cells build their subject on top of `Any`, so they need that family as well.
#[cfg(all(test, feature = "any"))]
mod tests;
