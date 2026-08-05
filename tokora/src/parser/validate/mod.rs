use crate::parse_state::ParseState;

use super::*;

/// A parser that validates output after parsing, preserving the original value on success.
///
/// This combinator applies **post-parse validation** similar to [`Filter`], checking
/// constraints on the successfully parsed value. The validator can reject invalid values
/// by returning an error, but cannot transform the value.
///
/// `Validate` is essentially the same as [`Filter`] - both validate without transformation.
/// The name `validate` may be preferred in some codebases for clarity.
///
/// # Type Parameters
///
/// - `P`: The inner parser to validate
/// - `F`: Validation function `FnMut(&O) -> Result<(), Error>`
/// - `O`: Output type (preserved if validation passes)
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Basic Validation
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Parse identifier and reject reserved keywords
/// let parser = parse_identifier()
///     .validate(|name| {
///         if KEYWORDS.contains(name.as_str()) {
///             Err(ReservedKeywordError::new())
///         } else {
///             Ok(())
///         }
///     });
///
/// // Input: "myVar"    → Ok("myVar")
/// // Input: "function" → Err(ReservedKeywordError)
/// ```
///
/// ## Numeric Range Validation
///
/// ```ignore
/// // Parse integer and validate range
/// let parser = parse_number()
///     .validate(|&n| {
///         if (0..=255).contains(&n) {
///             Ok(())
///         } else {
///             Err(OutOfRangeError::new(n, 0, 255))
///         }
///     });
/// ```
///
/// # Note
///
/// `Validate` and [`Filter`] are functionally identical. Choose whichever name
/// fits your codebase's terminology better:
/// - `.validate(|x| ...)` - Emphasizes the validation aspect
/// - `.filter(|x| ...)` - Familiar to Rust/functional programming users
///
/// # See Also
///
/// - [`Filter`] - Identical functionality, different name
/// - [`ValidateWith`] - Validate with access to parse state
/// - [`FilterMap`] - Validate and transform simultaneously
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Validate<P, F, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  validator: F,
  _l: PhantomData<L>,
  _o: PhantomData<O>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, F, O, L, Ctx, Lang: ?Sized, Cmpl> Validate<P, F, O, L, Ctx, Lang, Cmpl> {
  /// Creates a new `Validate` combinator for the specified language.
  #[inline(always)]
  pub(crate) const fn of<'inp>(parser: P, validator: F) -> Self
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    F: FnMut(&O) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  {
    Self {
      parser,
      validator,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _o: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, F, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for Validate<P, F, O, L, Ctx, Lang, Cmpl>
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
      .and_then(|output| (self.validator)(&output).map(|_| output))
  }
}

/// A parser that validates output with access to parse state (context, span, emitter).
///
/// This is the stateful variant of [`Validate`] that provides the validation function
/// with access to [`ParseState`], enabling context-aware validation based on:
/// - **Parse position**: Current cursor and span information
/// - **Parse context**: User-defined context data
/// - **Source text**: Via state.slice()
///
/// Use this when your validation logic needs information beyond the parsed value itself.
///
/// # Type Parameters
///
/// - `P`: The inner parser to validate
/// - `F`: Validation function `FnMut(&O, ParseState) -> Result<(), Error>`
/// - `O`: Output type (preserved if validation passes)
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Span-Aware Error Messages
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Validate with precise error location
/// let parser = parse_number()
///     .validate_with(|&n, state| {
///         if (0..=255).contains(&n) {
///             Ok(())
///         } else {
///             Err(OutOfRangeError::new(
///                 state.span(),
///                 state.slice().to_string(),
///                 n,
///                 0,
///                 255
///             ))
///         }
///     });
/// ```
///
/// ## Context-Based Validation
///
/// ```ignore
/// // Check for duplicate declarations
/// let parser = parse_identifier()
///     .validate_with(|name, state| {
///         if state.context().is_declared(name) {
///             Err(DuplicateIdentifierError::new(
///                 name.clone(),
///                 state.span()
///             ))
///         } else {
///             Ok(())
///         }
///     });
/// ```
///
/// # See Also
///
/// - [`Validate`] - Simpler validation without parse state access
/// - [`FilterWith`] - Identical functionality (different name)
/// - [`ParseState`] - The state object passed to validators
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ValidateWith<P, F, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  validator: F,
  _l: PhantomData<L>,
  _o: PhantomData<O>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, F, O, L, Ctx, Lang: ?Sized, Cmpl> ValidateWith<P, F, O, L, Ctx, Lang, Cmpl> {
  /// Creates a new `Validate` combinator for the specified language.
  #[inline(always)]
  pub(crate) const fn of<'inp>(parser: P, validator: F) -> Self
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
      validator,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _o: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, F, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for ValidateWith<P, F, O, L, Ctx, Lang, Cmpl>
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
    let start = input.cursor().clone();
    self
      .parser
      .parse_input(input)
      .and_then(|output| (self.validator)(&output, ParseState::new(input, start)).map(|_| output))
  }
}

// The cells build their subject on top of `Any`, so they need that family as well.
#[cfg(all(test, feature = "any"))]
mod tests;
