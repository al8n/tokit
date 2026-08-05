use super::*;

/// A parser that automatically consumes trivia (whitespace, comments) before and after parsing.
///
/// This combinator wraps a parser to handle **padding** - trivia tokens like whitespace and
/// comments that should be skipped. It consumes trivia both **before** and **after** the
/// inner parser runs, making the grammar whitespace-insensitive.
///
/// Trivia is determined by the lexer's [`Token::is_trivia()`](crate::Token::is_trivia)
/// method. Tokens marked as trivia are automatically skipped.
///
/// # Type Parameters
///
/// - `P`: The inner parser to wrap with padding
/// - `O`: Output type of the inner parser
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Basic Whitespace Handling
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Parse a number, ignoring surrounding whitespace
/// let parser = parse_number().padded();
///
/// // All of these parse successfully:
/// // Input: "42"      → Ok(42)
/// // Input: "  42"    → Ok(42)
/// // Input: "42  "    → Ok(42)
/// // Input: "  42  "  → Ok(42)
/// ```
///
/// ## Handling Comments
///
/// ```ignore
/// // Parse identifier with automatic comment skipping
/// let parser = parse_identifier().padded();
///
/// // Input: "foo"           → Ok("foo")
/// // Input: "/* c */ foo"   → Ok("foo")
/// // Input: "foo // comment\n" → Ok("foo")
/// // Input: "/* a */ foo /* b */" → Ok("foo")
/// ```
///
/// ## Sequential Parsing
///
/// ```ignore
/// // Parse: number + number with automatic whitespace handling
/// let parser = parse_number()
///     .padded()
///     .then_ignore(parse_plus())
///     .then(parse_number().padded());
///
/// // Input: "1+2"         → Ok((1, 2))
/// // Input: "1 + 2"       → Ok((1, 2))
/// // Input: "  1  +  2  " → Ok((1, 2))
/// ```
///
/// ## Building Whitespace-Insensitive Grammars
///
/// ```ignore
/// // Parse function call: identifier '(' args ')'
/// let parser = parse_identifier()
///     .padded()
///     .then_ignore(parse_lparen().padded())
///     .then(parse_args().padded())
///     .then_ignore(parse_rparen().padded());
///
/// // All equivalent:
/// // "foo(a,b)"
/// // "foo ( a , b )"
/// // "foo(\n  a,\n  b\n)"
/// ```
///
/// # How It Works
///
/// 1. **Skip leading trivia**: Consume all trivia tokens before the main parser
/// 2. **Parse**: Run the inner parser on the first non-trivia token
/// 3. **Skip trailing trivia**: Consume all trivia tokens after parsing
/// 4. **Return**: Return the parsed value
///
/// # Comparison with Padding Variants
///
/// | Combinator | Leading Trivia | Trailing Trivia | Use Case |
/// |------------|----------------|-----------------|----------|
/// | **Padded** | ✅ Skips | ✅ Skips | General whitespace handling |
/// | **PaddedLeft** | ✅ Skips | ❌ Keeps | Left-aligned tokens |
/// | **PaddedRight** | ❌ Keeps | ✅ Skips | Right-aligned tokens |
///
/// **When to use**:
/// - `Padded`: Most common - skip whitespace/comments on both sides
/// - `PaddedLeft`: When trailing whitespace matters (e.g., line-oriented syntax)
/// - `PaddedRight`: When leading whitespace matters (rare)
///
/// # Performance
///
/// - **Memory**: O(1) overhead
/// - **Runtime**: O(t) where t is the number of trivia tokens to skip
/// - **Streaming**: Trivia is consumed from the lexer lazily (not buffered)
///
/// # See Also
///
/// - [`PaddedLeft`] - Skip leading trivia only
/// - [`PaddedRight`] - Skip trailing trivia only
/// - [`Token::is_trivia()`](crate::Token::is_trivia) - Determines what counts as trivia
#[cfg_attr(
  feature = "then",
  doc = " - [`then_ignore`](crate::parser::ParseInput::then_ignore) - Ignore specific tokens (not trivia)"
)]
#[cfg_attr(
  not(feature = "then"),
  doc = " - `then_ignore` - Ignore specific tokens (not trivia)"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Padded<P, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  _m: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, O, L, Ctx, Lang: ?Sized, Cmpl> Padded<P, O, L, Ctx, Lang, Cmpl> {
  /// Creates a parser that accepts any token with optional padding.
  #[inline(always)]
  pub(crate) const fn new(parser: P) -> Self {
    Self {
      parser,
      _m: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for Padded<P, O, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
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
    Inner::new(&mut self.parser, Flavor::Both).parse_input(inp)
  }
}

/// A parser that automatically consumes trivia (whitespace, comments) before parsing only.
///
/// This is a variant of [`Padded`] that only skips **leading trivia**. Trailing trivia
/// after the parsed token is preserved and left for subsequent parsers.
///
/// Use this when:
/// - **Line-oriented syntax**: Newlines after tokens are significant
/// - **Indentation-sensitive languages**: Trailing whitespace affects layout
/// - **Asymmetric formatting**: Only left padding should be normalized
///
/// # Type Parameters
///
/// - `P`: The inner parser to wrap with left padding
/// - `O`: Output type of the inner parser
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Line-Oriented Parsing
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Parse statement but preserve trailing newline
/// let parser = parse_statement().padded_left();
///
/// // Input: "  print(x)\n"
/// //        ^^          ^ trailing newline preserved
/// //        leading whitespace skipped
/// ```
///
/// ## Preserving Line Breaks
///
/// ```ignore
/// // Parse identifier, allowing leading space but not trailing
/// let parser = parse_identifier().padded_left();
///
/// // Input: "  foo"     → Ok("foo"), remaining: ""
/// // Input: "  foo  "   → Ok("foo"), remaining: "  "
/// // Input: "  foo\n"   → Ok("foo"), remaining: "\n"
/// ```
///
/// # See Also
///
/// - [`Padded`] - Skip trivia on both sides (most common)
/// - [`PaddedRight`] - Skip trailing trivia only
/// - [`Token::is_trivia()`](crate::Token::is_trivia) - Determines trivia
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaddedLeft<P, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  _m: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, O, L, Ctx, Lang: ?Sized, Cmpl> PaddedLeft<P, O, L, Ctx, Lang, Cmpl> {
  /// Creates a parser that accepts any token with optional padding.
  #[inline(always)]
  pub(crate) const fn new(parser: P) -> Self {
    Self {
      parser,
      _m: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for PaddedLeft<P, O, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
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
    Inner::new(&mut self.parser, Flavor::Leading).parse_input(inp)
  }
}

/// A parser that automatically consumes trivia (whitespace, comments) after parsing only.
///
/// This is a variant of [`Padded`] that only skips **trailing trivia**. Leading trivia
/// before the token must be handled by previous parsers or is treated as significant.
///
/// Use this when:
/// - **Leading whitespace is significant**: Indentation or alignment matters
/// - **Right-aligned tokens**: Trailing space should be normalized
/// - **Asymmetric formatting**: Only right padding should be ignored
///
/// # Type Parameters
///
/// - `P`: The inner parser to wrap with right padding
/// - `O`: Output type of the inner parser
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Preserving Leading Whitespace
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Parse token but preserve leading indentation
/// let parser = parse_identifier().padded_right();
///
/// // Input: "  foo"     → Err (leading space not skipped)
/// // Input: "foo  "     → Ok("foo"), trailing space skipped
/// // Input: "foo\n"     → Ok("foo"), trailing newline skipped
/// ```
///
/// ## Right-Aligned Cleanup
///
/// ```ignore
/// // Parse number and clean up trailing whitespace
/// let parser = parse_number().padded_right();
///
/// // Input: "42  "   → Ok(42), remaining: ""
/// // Input: "42\n\n" → Ok(42), remaining: ""
/// ```
///
/// # See Also
///
/// - [`Padded`] - Skip trivia on both sides (most common)
/// - [`PaddedLeft`] - Skip leading trivia only
/// - [`Token::is_trivia()`](crate::Token::is_trivia) - Determines trivia
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaddedRight<P, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  _m: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, O, L, Ctx, Lang: ?Sized, Cmpl> PaddedRight<P, O, L, Ctx, Lang, Cmpl> {
  /// Creates a parser that accepts any token with optional padding.
  #[inline(always)]
  pub(crate) const fn new(parser: P) -> Self {
    Self {
      parser,
      _m: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for PaddedRight<P, O, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
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
    Inner::new(&mut self.parser, Flavor::Trailing).parse_input(inp)
  }
}

enum Flavor {
  Leading,
  Trailing,
  Both,
}

impl Flavor {
  #[inline(always)]
  const fn clear_leading(&self) -> bool {
    matches!(self, Flavor::Leading | Flavor::Both)
  }

  #[inline(always)]
  const fn is_trailing(&self) -> bool {
    matches!(self, Flavor::Trailing | Flavor::Both)
  }
}

struct Inner<P, O, L, Ctx, Lang: ?Sized = ()> {
  parser: P,
  flavor: Flavor,
  _m: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
}

impl<P, O, L, Ctx, Lang: ?Sized> Inner<P, O, L, Ctx, Lang> {
  #[inline(always)]
  const fn new(parser: P, flavor: Flavor) -> Self {
    Self {
      parser,
      flavor,
      _m: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
    }
  }
}

impl<'inp, P, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for Inner<&mut P, O, L, Ctx, Lang>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
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
    if self.flavor.clear_leading() {
      inp.skip_while(|t| t.is_trivia())?;
    }
    let output = self.parser.parse_input(inp)?;
    if self.flavor.is_trailing() {
      inp.skip_while(|t| t.is_trivia())?;
    }
    Ok(output)
  }
}
