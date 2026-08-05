use super::*;

/// A parser that unwraps `Option<T>` output to `T`, panicking on `None`.
///
/// This combinator converts a parser that returns `Option<O>` into one that returns `O`
/// by calling `.unwrap()` on the result. If the inner parser returns `None`, this will
/// **panic** with a message indicating the source location of the `.unwrapped()` call.
///
/// # When to Use
///
/// Use this when:
/// - You've already validated that a value must be present
/// - During development/debugging to find logic errors
/// - In situations where `None` represents a programming error, not a parsing error
///
/// **Warning**: This is similar to `.unwrap()` in Rust - it will panic if the value is `None`.
#[cfg_attr(
  feature = "filter",
  doc = " For error handling, prefer using [`filter_map`](FilterMap) or working with `Option` directly."
)]
#[cfg_attr(
  not(feature = "filter"),
  doc = " For error handling, prefer using `filter_map` or working with `Option` directly."
)]
///
/// # Type Parameters
///
/// - `P`: The inner parser (must output `Option<O>`)
/// - `O`: The unwrapped output type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Basic Usage
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Parse an optional element, then unwrap it
/// let parser = optional_element()
///     .unwrapped();  // Panics if None
///
/// // This is equivalent to:
/// let parser = optional_element()
///     .map(|opt| opt.unwrap());
/// ```
///
/// # Panics
///
/// Panics if the inner parser returns `Ok(None)`. The panic message includes the
/// source location of the `.unwrapped()` call (tracked via `#[track_caller]`).
///
/// # See Also
///
#[cfg_attr(
  feature = "filter",
  doc = " - [`filter_map`](FilterMap) - Transform and filter with error handling"
)]
#[cfg_attr(
  not(feature = "filter"),
  doc = " - `filter_map` - Transform and filter with error handling"
)]
#[cfg_attr(
  feature = "map",
  doc = " - [`map`](Map) - Transform output without unwrapping"
)]
#[cfg_attr(
  not(feature = "map"),
  doc = " - `map` - Transform output without unwrapping"
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Unwrapped<P, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  pub(crate) parser: P,
  pub(crate) _m: PhantomData<O>,
  pub(crate) _ctx: PhantomData<Ctx>,
  pub(crate) _lang: PhantomData<Lang>,
  pub(crate) _cmpl: PhantomData<Cmpl>,
}

impl<P, O, Ctx, Lang: ?Sized, Cmpl> Unwrapped<P, O, Ctx, Lang, Cmpl> {
  /// Creates a new `Unwrapped` parser.
  #[inline(always)]
  #[track_caller]
  pub(crate) const fn new(parser: P) -> Self {
    Self {
      parser,
      _m: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for Unwrapped<P, O, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: ParseInput<'inp, L, Option<O>, Ctx, Lang, Cmpl>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self.parser.parse_input(inp).map(Option::unwrap)
  }
}
