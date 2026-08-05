use super::*;

/// A parser that runs another parser but discards its output, returning `()`.
///
/// This combinator executes a parser for its **side effects** (consuming tokens, emitting
/// errors, advancing position) while throwing away the parsed value. It's primarily used
/// via the convenient `.ignore()`, `.then_ignore()`, and `.ignore_then()` methods.
///
/// # Type Parameters
///
/// - `P`: The inner parser whose output will be ignored
/// - `O`: The original output type (discarded)
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Ignore Separators
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Parse number, comma, number - keep only numbers
/// let parser = parse_number()
///     .then_ignore(parse_comma())  // Uses Ignore internally
///     .then(parse_number());
///
/// // Input: "1, 2"  → Ok((1, 2))
/// // The comma is parsed but discarded
/// ```
///
/// ## Ignore Delimiters
///
/// ```ignore
/// // Parse parenthesized expression: (expr)
/// let parser = parse_lparen()
///     .ignore_then(parse_expression())  // Uses Ignore internally
///     .then_ignore(parse_rparen());
///
/// // Input: "(foo + bar)"  → Ok(Expression)
/// // Parentheses are parsed but discarded
/// ```
///
/// ## Explicit Ignore
///
/// ```ignore
/// // Explicitly ignore a parser's output
/// let parser = parse_comment()
///     .ignore()
///     .then(parse_statement());
///
/// // Equivalent to:
/// let parser = parse_comment()
///     .map(|_| ())
///     .then(parse_statement());
/// ```
///
/// ## Multiple Ignored Parsers
///
/// ```ignore
/// // Parse: fn name(args) { ... }
/// //        ^^ ignore  ^ ignore
/// let parser = parse_fn_keyword()
///     .ignore_then(parse_identifier())
///     .then_ignore(parse_lparen())
///     .then(parse_args())
///     .then_ignore(parse_rparen())
///     .then_ignore(parse_block());
///
/// // Returns just: (name, args)
/// ```
///
/// # When to Use
///
/// - **Separators**: Commas, semicolons between elements
/// - **Delimiters**: Parentheses, brackets, braces around content
/// - **Keywords**: Reserved words that mark structure but aren't data
/// - **Whitespace**: Explicit whitespace handling (though `.padded()` is usually better)
///
/// **Prefer**:
/// - `.then_ignore(p)` over `.then(p).map(|(a, _)| a)`
/// - `.ignore_then(p)` over `.then(p).map(|(_, b)| b)`
/// - `.padded()` over manually ignoring whitespace
///
/// # Comparison with Map
///
/// | Combinator | Transformation | Use Case |
/// |------------|---------------|----------|
/// | **Map** | `T -> U` | Transform value to new type |
/// | **Ignore** | `T -> ()` | Discard value, keep side effects |
///
/// `Ignore` is just `map(|_| ())` with a clearer name.
///
/// # Performance
///
/// - **Memory**: O(1) overhead
/// - **Runtime**: Same as the inner parser (just discards the result)
/// - **Zero-cost**: Optimizer typically eliminates the ignored value entirely
///
/// # See Also
///
#[cfg_attr(
  feature = "then",
  doc = " - [`then_ignore`](crate::parser::ParseInput::then_ignore) - Parse then discard second"
)]
#[cfg_attr(
  not(feature = "then"),
  doc = " - `then_ignore` - Parse then discard second"
)]
#[cfg_attr(
  feature = "then",
  doc = " - [`ignore_then`](crate::parser::ParseInput::ignore_then) - Parse then discard first"
)]
#[cfg_attr(
  not(feature = "then"),
  doc = " - `ignore_then` - Parse then discard first"
)]
#[cfg_attr(
  feature = "map",
  doc = " - [`Map`] - Transform output instead of discarding"
)]
#[cfg_attr(
  not(feature = "map"),
  doc = " - `Map` - Transform output instead of discarding"
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ignore<P, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  _output: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, O, L, Ctx, Lang: ?Sized, Cmpl> Ignore<P, O, L, Ctx, Lang, Cmpl> {
  /// Creates a parser that ignores any output.
  #[inline(always)]
  pub(crate) const fn new(parser: P) -> Self {
    Self {
      parser,
      _output: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, (), Ctx, Lang, Cmpl>
  for Ignore<P, O, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self.parser.parse_input(inp).map(|_| ())
  }
}
