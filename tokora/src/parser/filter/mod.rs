use crate::parse_state::ParseState;

use super::*;

/// A parser that validates output after parsing, rejecting invalid values with errors.
///
/// This combinator applies **post-parse validation** by running a validation function on
/// the successfully parsed output. The original value is preserved if validation passes;
/// otherwise, an error is returned.
///
/// Unlike [`PeekThen`] which validates **before** parsing using lookahead, `Filter` validates
/// **after** parsing when you have the full parsed value. This is useful for:
/// - **Semantic validation**: Checking constraints that can't be determined from lookahead
/// - **Value-based rejection**: Filtering based on computed properties
/// - **Context-aware errors**: Reporting specific validation failures
///
/// # Type Parameters
///
/// - `P`: The inner parser to validate
/// - `F`: Validation function `FnMut(&O) -> Result<(), Error>`
/// - `O`: Output type of the inner parser (preserved if validation passes)
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Basic Validation
///
/// ```ignore
/// use tokora::parser::{any, ParseInput};
///
/// // Parse any token, but only accept identifiers
/// let parser = any::<MyLexer>()
///     .filter(|tok| {
///         if tok.is_identifier() {
///             Ok(())
///         } else {
///             Err(UnexpectedTokenKind::new(tok.kind()))
///         }
///     });
///
/// // Input: Identifier("foo") → Ok(Identifier("foo"))
/// // Input: Number(42)        → Err(UnexpectedTokenKind)
/// ```
///
/// ## Numeric Constraints
///
/// ```ignore
/// // Parse a number, but only accept even numbers
/// let parser = parse_number()
///     .filter(|&n| {
///         if n % 2 == 0 {
///             Ok(())
///         } else {
///             Err(OddNumberError::new())
///         }
///     });
///
/// // Input: "42"  → Ok(42)
/// // Input: "43"  → Err(OddNumberError)
/// ```
///
/// ## Range Validation
///
/// ```ignore
/// // Parse an integer within a specific range
/// let parser = parse_int()
///     .filter(|&n| {
///         if (0..=100).contains(&n) {
///             Ok(())
///         } else {
///             Err(OutOfRangeError::new(n, 0, 100))
///         }
///     });
/// ```
///
/// ## String Pattern Validation
///
/// ```ignore
/// // Parse identifier, but reject reserved keywords
/// let parser = parse_identifier()
///     .filter(|name| {
///         if RESERVED_KEYWORDS.contains(name.as_str()) {
///             Err(ReservedKeywordError::new(name.clone()))
///         } else {
///             Ok(())
///         }
///     });
///
/// // Input: "myVar"    → Ok("myVar")
/// // Input: "function" → Err(ReservedKeywordError)
/// ```
///
/// # How It Works
///
/// 1. **Parse**: Inner parser runs and produces `Result<O, E>`
/// 2. **Validate**: If parsing succeeded, validator runs on the output
/// 3. **Decision**:
///    - If validator returns `Ok(())`: original value is returned
///    - If validator returns `Err(e)`: error is propagated
///
/// # Comparison with Related Combinators
///
/// | Combinator | When Validates | Transforms Value | Use Case |
/// |------------|----------------|------------------|----------|
/// | **Filter** | After parsing | ❌ No (preserves value) | Reject invalid values |
/// | **FilterMap** | After parsing | ✅ Yes (transforms) | Validate + transform |
/// | **Map** | After parsing | ✅ Always succeeds | Transform only |
/// | **PeekThen** | Before parsing | ❌ No | Reject via lookahead |
///
/// **When to use each**:
/// - `Filter`: Validate without changing the value (e.g., range checks, pattern matching)
/// - `FilterMap`: Validate and transform simultaneously (e.g., parse string to int)
/// - `Map`: Transform without possibility of failure (e.g., wrap in enum variant)
/// - `PeekThen`: Decide whether to parse based on lookahead tokens
///
/// # Performance
///
/// - **Memory**: O(1) overhead (just the validator closure)
/// - **Runtime**: Single validation check after parsing
/// - **No backtracking**: If validation fails, parsing doesn't retry
///
/// # See Also
///
/// - [`FilterMap`] - Validate and transform output
/// - [`FilterWith`] - Validate with access to parse state
/// - [`PeekThen`] - Validate before parsing using lookahead
/// - [`Map`] - Transform output without validation
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Filter<P, F, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  filter: F,
  _marker: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, F, O, L, Ctx, Lang: ?Sized, Cmpl> Filter<P, F, O, L, Ctx, Lang, Cmpl> {
  /// Creates a new `Validate` combinator for the specified language.
  #[inline(always)]
  pub(crate) const fn of<'inp>(parser: P, filter: F) -> Self
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    F: FnMut(&O) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  {
    Self {
      parser,
      filter,
      _marker: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, F, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for Filter<P, F, O, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  F: FnMut(&O) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self
      .parser
      .parse_input(input)
      .and_then(|output| (self.filter)(&output).map(|_| output))
  }
}

/// A parser that validates output with access to parse state (cursor, emitter, context).
///
/// This is the stateful variant of [`Filter`] that provides the validation function
/// with access to [`ParseState`], allowing for context-aware validation based on:
/// - **Current parse position**: Via the cursor
/// - **Parse context**: Access to user-defined context data
/// - **Emitter**: For complex error reporting
///
/// Use this when your validation logic needs information beyond the parsed value itself.
///
/// # Type Parameters
///
/// - `P`: The inner parser to validate
/// - `F`: Validation function `FnMut(&O, ParseState) -> Result<(), Error>`
/// - `O`: Output type of the inner parser
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Position-Aware Validation
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Reject duplicate identifiers by checking parse context
/// let parser = parse_identifier()
///     .filter_with(|name, state| {
///         if state.context().is_declared(name) {
///             Err(DuplicateIdentifierError::new(name.clone()))
///         } else {
///             Ok(())
///         }
///     });
/// ```
///
/// ## Span-Based Error Reporting
///
/// ```ignore
/// // Validate with precise span information
/// let parser = parse_literal()
///     .filter_with(|lit, state| {
///         if lit.is_too_large() {
///             let span = state.span();
///             Err(LiteralOverflowError::new(*span, lit.clone()))
///         } else {
///             Ok(())
///         }
///     });
/// ```
///
/// # See Also
///
/// - [`Filter`] - Simpler validation without parse state access
/// - [`FilterMapWith`] - Transform and validate with parse state
/// - [`ParseState`] - The state object passed to validators
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FilterWith<P, F, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  filter: F,
  _marker: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, F, O, L, Ctx, Lang: ?Sized, Cmpl> FilterWith<P, F, O, L, Ctx, Lang, Cmpl> {
  /// Creates a new `Validate` combinator for the specified language.
  #[inline(always)]
  pub(crate) const fn of<'inp>(parser: P, filter: F) -> Self
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    F: FnMut(
      &O,
      ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
    ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  {
    Self {
      parser,
      filter,
      _marker: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, F, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for FilterWith<P, F, O, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  F: FnMut(
    &O,
    ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let cursor = input.cursor().clone();
    self
      .parser
      .parse_input(input)
      .and_then(|output| (self.filter)(&output, ParseState::new(input, cursor)).map(|_| output))
  }
}

// The cells build their subject on top of `Any`, so they need that family as well.
#[cfg(all(test, feature = "any"))]
mod tests;
