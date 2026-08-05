use core::marker::PhantomData;

use crate::parse_state::ParseState;

use super::*;

/// A parser that transforms and validates output, combining mapping with fallible conversion.
///
/// This combinator performs **post-parse transformation with validation** by running a
/// function that can both transform the value and reject invalid inputs. It's the fallible
/// counterpart to [`Map`].
///
/// Unlike [`Filter`] which preserves the original value, `FilterMap` produces a **new value**
/// of a potentially different type. Unlike [`Map`] which always succeeds, `FilterMap` can
/// **fail** with an error.
///
/// This is particularly useful for:
/// - **Type conversions**: Parse string to int, validate enum variants, etc.
/// - **Extraction**: Pull out specific fields or properties with validation
/// - **Conditional transformation**: Transform only if conditions are met
///
/// # Type Parameters
///
/// - `P`: The inner parser
/// - `F`: Transformation function `FnMut(O) -> Result<U, Error>`
/// - `O`: Input type from the inner parser
/// - `U`: Output type after transformation
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## String to Integer Conversion
///
/// ```ignore
/// use tokora::parser::{any, ParseInput};
///
/// // Parse a string token and convert to integer
/// let parser = parse_string()
///     .filter_map(|s| {
///         s.parse::<i32>()
///             .map_err(|_| InvalidNumberError::new())
///     });
///
/// // Input: "123"  → Ok(123)
/// // Input: "abc"  → Err(InvalidNumberError)
/// ```
///
/// ## Enum Variant Validation
///
/// ```ignore
/// // Parse a token and convert to enum, rejecting unknown variants
/// let parser = parse_identifier()
///     .filter_map(|name| {
///         match name.as_str() {
///             "true" => Ok(Keyword::True),
///             "false" => Ok(Keyword::False),
///             "null" => Ok(Keyword::Null),
///             _ => Err(UnknownKeywordError::new(name)),
///         }
///     });
///
/// // Input: "true"  → Ok(Keyword::True)
/// // Input: "foo"   → Err(UnknownKeywordError)
/// ```
///
/// ## Field Extraction with Validation
///
/// ```ignore
/// // Extract and validate a specific field from a token
/// let parser = any::<MyLexer>()
///     .filter_map(|tok| {
///         match tok {
///             Token::Identifier(name) if !name.is_empty() => Ok(name),
///             Token::Identifier(_) => Err(EmptyIdentifierError::new()),
///             _ => Err(ExpectedIdentifierError::new()),
///         }
///     });
/// ```
///
/// ## Option Unwrapping with Error
///
/// ```ignore
/// // Parse and unwrap an Option, failing if None
/// let parser = parse_optional_value()
///     .filter_map(|opt| {
///         opt.ok_or_else(|| MissingValueError::new())
///     });
///
/// // Input with value: Ok(value)
/// // Input without:    Err(MissingValueError)
/// ```
///
/// # How It Works
///
/// 1. **Parse**: Inner parser runs and produces `Result<O, E>`
/// 2. **Transform**: If parsing succeeded, mapper runs on the output
/// 3. **Decision**:
///    - If mapper returns `Ok(new_value)`: new value is returned
///    - If mapper returns `Err(e)`: error is propagated
///
/// # Comparison with Related Combinators
///
/// | Combinator | Transforms | Can Fail | Use Case |
/// |------------|-----------|----------|----------|
/// | **Map** | ✅ Yes | ❌ Never | Infallible transformations |
/// | **Filter** | ❌ No | ✅ Yes | Validation only |
/// | **FilterMap** | ✅ Yes | ✅ Yes | Fallible transformations |
///
/// **When to use each**:
/// - `Map`: Transform without possibility of failure (e.g., `|x| x * 2`)
/// - `Filter`: Validate without changing the value (e.g., range checks)
/// - `FilterMap`: Transform with validation (e.g., string parsing, enum conversion)
///
/// # Common Patterns
///
/// ## Chaining with Filter
/// ```ignore
/// // First transform, then validate
/// let parser = parse_token()
///     .filter_map(|tok| tok.try_as_number())  // Option<i32> → Result<i32>
///     .filter(|&n| {                           // Validate range
///         if (0..100).contains(&n) {
///             Ok(())
///         } else {
///             Err(OutOfRangeError::new(n))
///         }
///     });
/// ```
///
/// # Performance
///
/// - **Memory**: O(1) overhead (just the mapper closure)
/// - **Runtime**: Single transformation after parsing
/// - **No backtracking**: If transformation fails, parsing doesn't retry
///
/// # See Also
///
/// - [`Map`] - Infallible transformation
/// - [`Filter`] - Validation without transformation
/// - [`FilterMapWith`] - Transform with access to parse state
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FilterMap<P, F, O, U, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  mapper: F,
  _o: PhantomData<O>,
  _u: PhantomData<U>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, F, O, U, L, Ctx, Lang: ?Sized, Cmpl> FilterMap<P, F, O, U, L, Ctx, Lang, Cmpl> {
  /// Creates a new `Validate` combinator for the specified language.
  #[inline(always)]
  pub(crate) const fn of<'inp>(parser: P, filter: F) -> Self
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    F: FnMut(O) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  {
    Self {
      parser,
      mapper: filter,
      _o: PhantomData,
      _u: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, F, L, O, U, Ctx, Lang, Cmpl> ParseInput<'inp, L, U, Ctx, Lang, Cmpl>
  for FilterMap<P, F, O, U, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  F: FnMut(O) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.parser.parse_input(input).and_then(&mut self.mapper)
  }
}

/// A parser that transforms and validates output with access to parse state.
///
/// This is the stateful variant of [`FilterMap`] that provides the transformation function
/// with access to [`ParseState`], enabling context-aware fallible transformations based on:
/// - **Current parse position**: Via the cursor
/// - **Parse context**: Access to user-defined context data
/// - **Span information**: For detailed error reporting
///
/// Use this when your transformation or validation logic needs information beyond the
/// parsed value itself, such as position information or parse context.
///
/// # Type Parameters
///
/// - `P`: The inner parser
/// - `F`: Transformation function `FnMut(O, ParseState) -> Result<U, Error>`
/// - `O`: Input type from the inner parser
/// - `U`: Output type after transformation
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Span-Aware Type Conversion
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Parse a string and convert to integer with span in error
/// let parser = parse_string()
///     .filter_map_with(|s, state| {
///         s.parse::<i32>()
///             .map_err(|_| InvalidNumberError::new(state.span()))
///     });
///
/// // Errors include exact span of the invalid number
/// ```
///
/// ## Context-Based Transformation
///
/// ```ignore
/// // Transform identifier based on context (e.g., resolve aliases)
/// let parser = parse_identifier()
///     .filter_map_with(|name, state| {
///         state.context()
///             .resolve_alias(&name)
///             .ok_or_else(|| UnknownIdentifierError::new(name, state.span()))
///     });
/// ```
///
/// ## Position-Dependent Validation
///
/// ```ignore
/// // Only allow certain constructs in specific contexts
/// let parser = parse_expression()
///     .filter_map_with(|expr, state| {
///         if state.context().allows_async() {
///             Ok(expr)
///         } else if expr.contains_await() {
///             Err(AwaitOutsideAsyncError::new(state.span()))
///         } else {
///             Ok(expr)
///         }
///     });
/// ```
///
/// # See Also
///
/// - [`FilterMap`] - Simpler transformation without parse state access
/// - [`FilterWith`] - Validate (not transform) with parse state
/// - [`ParseState`] - The state object passed to transformers
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FilterMapWith<P, F, O, U, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  mapper: F,
  _o: PhantomData<O>,
  _u: PhantomData<U>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, F, O, U, L, Ctx, Lang: ?Sized, Cmpl> FilterMapWith<P, F, O, U, L, Ctx, Lang, Cmpl> {
  /// Creates a new `Validate` combinator for the specified language.
  #[inline(always)]
  pub(crate) const fn of<'inp>(parser: P, filter: F) -> Self
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    F: FnMut(
      O,
      ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
    ) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  {
    Self {
      parser,
      mapper: filter,
      _o: PhantomData,
      _u: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, F, L, O, U, Ctx, Lang, Cmpl> ParseInput<'inp, L, U, Ctx, Lang, Cmpl>
  for FilterMapWith<P, F, O, U, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  F: FnMut(
    O,
    ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let start = input.cursor().clone();
    self
      .parser
      .parse_input(input)
      .and_then(|output| (self.mapper)(output, ParseState::new(input, start)))
  }
}

// The cells build their subject on top of `Any`, so they need that family as well.
#[cfg(all(test, feature = "any"))]
mod tests;
