use super::*;

/// A parser that always succeeds without consuming input, returning `()`.
///
/// This parser is the identity element for parser sequencing - it does nothing,
/// consumes nothing, and always succeeds immediately. It's useful for:
/// - **Default values**: Optional parsers that default to doing nothing
/// - **Conditional logic**: Placeholder in choice combinators
/// - **Testing**: Minimal parser for test scaffolding
///
/// # Examples
///
/// ## Placeholder in Choices
///
/// ```ignore
/// // Parse statement: either a let-binding or nothing
/// let parser = peek_then_choice((
///     parse_let_stmt(),
///     empty().map(|_| Statement::Empty),
/// ));
/// ```
///
/// ## Default Behavior
///
/// ```ignore
/// // Conditional parsing
/// let parser = if needs_prefix {
///     parse_prefix().map(Some)
/// } else {
///     empty().map(|_| None)
/// };
/// ```
///
/// ## Testing
///
/// ```ignore
/// // Minimal parser for testing combinators
/// #[test]
/// fn test_map_combinator() {
///     let parser = empty().map(|_| 42);
///     assert_eq!(parser.parse(""), Ok(42));
/// }
/// ```
///
/// # When to Use
///
/// - **Defaults**: When an optional parser should default to no-op
/// - **Type matching**: When you need a parser that returns `()` for sequencing
/// - **Placeholders**: In development before implementing the real parser
///
/// **Don't use** when you actually want to fail - use a parser that returns an error instead.
///
/// # Performance
///
/// - **Memory**: Zero-sized type
/// - **Runtime**: O(1) - instant success, no input consumption
///
/// # See Also
///
/// - [`Todo`] - Placeholder that panics (for unimplemented parsers)
#[cfg_attr(
  feature = "map",
  doc = " - [`map`](crate::parser::ParseInput::map) - Transform `()` to other types"
)]
#[cfg_attr(not(feature = "map"), doc = " - `map` - Transform `()` to other types")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Empty(());

impl Empty {
  /// Creates a parser that always succeeds without consuming any input.
  #[inline(always)]
  pub const fn new() -> Self {
    Self(())
  }
}

impl<'inp, L, Ctx, Lang, Cmpl> ParseInput<'inp, L, (), Ctx, Lang, Cmpl> for Empty
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    _inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    Ok(())
  }
}
