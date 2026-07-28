use core::marker::PhantomData;

use derive_more::IsVariant;

use crate::{delimiter::Delimiter, punct::Punctuator};

use super::*;

mod parse;

mod delim;

/// A parser that parses a sequence of elements separated by a delimiter.
///
/// This combinator parses repeated occurrences of an element parser, expecting each
/// element to be separated by a delimiter (e.g., comma, semicolon). It provides
/// fine-grained control over:
/// - **Leading separators**: Allow/deny/require separators before the first element
/// - **Trailing separators**: Allow/deny/require separators after the last element
/// - **Repetition bounds**: Minimum and maximum number of elements
///
/// # Type Parameters
///
/// - `F`: The element parser
/// - `Sep`: Separator checker (e.g., comma punctuator, custom classifier)
/// - `Condition`: Decision function that determines when to stop parsing
/// - `O`: Output type of the element parser
/// - `Window`: Lookahead window size for the condition
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Config`: Configuration options (trailing/leading/min/max)
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Basic Comma-Separated List
///
/// ```ignore
/// use tokora::parser::{SeparatedWhile, ParseInput};
/// use generic_arraydeque::typenum::U1;
///
/// // Parse: element, element, element
/// let parser = SeparatedWhile::comma::<MyLexer, U1, Ctx>(
///     element_parser(),
///     |peeked, _| match peeked.front() {
///         None => Ok(Action::Stop),
///         Some(Token::Comma) => Ok(Action::Continue),
///         _ => Ok(Action::Stop),
///     }
/// ).collect::<Vec<_>>();
///
/// // Input: "1, 2, 3"
/// // Output: Ok(vec![1, 2, 3])
/// ```
///
/// ## With Trailing Separator
///
/// ```ignore
/// // Parse: element, element, element,  (trailing comma allowed)
/// let parser = SeparatedWhile::comma::<MyLexer, U1, Ctx>(
///     element_parser(),
///     stop_condition
/// )
/// .allow_trailing()   // Allow trailing comma
/// .collect::<Vec<_>>();
///
/// // Input: "1, 2, 3,"
/// // Output: Ok(vec![1, 2, 3])
/// ```
///
/// ## With Leading Separator
///
/// ```ignore
/// // Parse: , element, element  (leading comma allowed)
/// let parser = SeparatedWhile::comma::<MyLexer, U1, Ctx>(
///     element_parser(),
///     stop_condition
/// )
/// .allow_leading()    // Allow leading comma
/// .collect::<Vec<_>>();
///
/// // Input: ", 1, 2"
/// // Output: Ok(vec![1, 2])
/// ```
///
/// ## With Bounds
///
/// ```ignore
/// // Parse at least 1, at most 5 elements
/// let parser = SeparatedWhile::comma::<MyLexer, U1, Ctx>(
///     element_parser(),
///     stop_condition
/// )
/// .at_least(Minimum::new(1))
/// .at_most(Maximum::new(5))
/// .collect::<Vec<_>>();
/// ```
///
/// ## Custom Separator
///
/// ```ignore
/// // Parse elements separated by semicolons
/// let parser = SeparatedWhile::semicolon::<MyLexer, U1, Ctx>(
///     element_parser(),
///     stop_condition
/// ).collect::<Vec<_>>();
///
/// // Input: "a;b;c"
/// // Output: Ok(vec![a, b, c])
/// ```
///
/// # How It Works
///
/// 1. **Parse first element** (unless leading separator is required)
/// 2. **Loop**:
///    - Call `condition` to check if we should continue
///    - If `Action::Continue`: parse separator, then element
///    - If `Action::Stop`: break
/// 3. **Validate** trailing separator rules
/// 4. **Collect** parsed elements into container
///
/// # Error Handling
///
/// The parser emits errors via the [`SeparatedEmitter`](crate::emitter::SeparatedEmitter) trait:
/// - Missing separator between elements
/// - Unexpected leading separator (when denied)
/// - Unexpected trailing separator (when denied)
/// - Missing element after separator
/// - Too few or too many elements (when bounds set)
///
/// # Performance
///
/// - **Memory**: O(1) for the parser itself (elements collected into container)
/// - **Parsing**: O(n) where n is the number of elements
/// - **Lookahead**: O(W) per iteration where W is the window size
///
/// # See Also
///
/// - [`delimited`](SeparatedWhile::delimited) - Wrap in delimiters (e.g., `[...]` or `{...}`)
/// - [`repeated`](RepeatedWhile) - Repeat without separators
/// - [`Collect`](crate::parser::Collect) - Wrapper for collecting elements into a container
///
/// # Completeness (0.3.0): Complete-only
///
/// Decision-window class: the `Decision` peeks a
/// `W`-window, and at a non-final Partial frontier the peek fill silently serves a SHORT
/// window (the peek contract: short at the frontier, never an error). The condition would
/// read that truncation as "construct ended" and return `Ok` early — breaking chunked
/// equivalence with no error on any channel. Generalizing needs the deferred
/// frontier-window rule (full-or-incomplete decision windows); until then the impls stay
/// pinned at `Complete` in both positions, so a Partial drive is a compile-time wall.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct SeparatedWhile<F, Sep, Condition, O, Window, L, Ctx, Lang: ?Sized = (), Cmpl = Complete>
{
  pub(super) f: F,
  pub(super) condition: Condition,
  pub(super) _sep: PhantomData<Sep>,
  pub(super) _m: PhantomData<O>,
  pub(super) _decision_window: PhantomData<Window>,
  pub(super) _l: PhantomData<L>,
  pub(super) _ctx: PhantomData<Ctx>,
  pub(super) _lang: PhantomData<Lang>,
  pub(super) _cmpl: PhantomData<Cmpl>,
}

impl<F, Sep, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl> Copy
  for SeparatedWhile<F, Sep, Condition, O, W, L, Ctx, Lang, Cmpl>
where
  F: Copy,
  Sep: Copy,
  Condition: Copy,
{
}

impl<F, Sep, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl> Clone
  for SeparatedWhile<F, Sep, Condition, O, W, L, Ctx, Lang, Cmpl>
where
  F: Clone,
  Sep: Clone,
  Condition: Clone,
{
  #[inline(always)]
  fn clone(&self) -> Self {
    Self {
      f: self.f.clone(),
      condition: self.condition.clone(),
      _sep: PhantomData,
      _m: PhantomData,
      _decision_window: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  SeparatedWhile<F, (), Condition, O, W, L, Ctx, Lang, Cmpl>
{
  /// Creates a new `SeparatedWhile` parser with the given container.
  #[inline(always)]
  pub const fn new<Sep>(
    f: F,
    condition: Condition,
  ) -> SeparatedWhile<F, Sep, Condition, O, W, L, Ctx, Lang, Cmpl> {
    SeparatedWhile {
      f,
      condition,
      _sep: PhantomData,
      _m: PhantomData,
      _decision_window: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<F, Sep, Condition, O, Window, L, Ctx, Lang: ?Sized, Cmpl>
  SeparatedWhile<F, Sep, Condition, O, Window, L, Ctx, Lang, Cmpl>
{
  /// Creates a mutable reference version of this `SeparatedWhile` parser.
  #[inline(always)]
  pub const fn as_mut(
    &mut self,
  ) -> SeparatedWhile<&mut F, Sep, &mut Condition, O, Window, L, Ctx, Lang, Cmpl> {
    SeparatedWhile {
      f: &mut self.f,
      condition: &mut self.condition,
      _sep: PhantomData,
      _m: PhantomData,
      _decision_window: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }

  /// Returns a mutable reference to the inner parser function.
  #[inline(always)]
  pub fn fn_mut(&mut self) -> &mut F {
    &mut self.f
  }

  /// Returns mutable references to the inner parser function and condition.
  #[inline(always)]
  pub fn parts_mut(&mut self) -> (&mut F, &mut Condition) {
    (&mut self.f, &mut self.condition)
  }

  /// Sets the minimum number of elements to parse.
  #[inline(always)]
  pub const fn at_least(self, minimum: usize) -> AtLeast<Self> {
    AtLeast::new(self, minimum)
  }

  /// Sets the maximum number of elements to parse.
  #[inline(always)]
  pub const fn at_most(self, maximum: usize) -> AtMost<Self> {
    AtMost::new(self, maximum)
  }

  /// Sets both the minimum and maximum number of elements to parse.
  #[inline(always)]
  pub const fn bounded(self, minimum: usize, maximum: usize) -> Bounded<Self> {
    Bounded::new(self, maximum, minimum)
  }

  /// Sets allows trailing separator.
  #[inline(always)]
  pub const fn allow_trailing(self) -> AllowTrailing<Self> {
    AllowTrailing::new(self)
  }

  /// Sets requires trailing separator.
  #[inline(always)]
  pub const fn require_trailing(self) -> RequireTrailing<Self> {
    RequireTrailing::new(self)
  }

  /// Sets allows leading separator.
  #[inline(always)]
  pub const fn allow_leading(self) -> AllowLeading<Self> {
    AllowLeading::new(self)
  }

  /// Sets requires leading separator.
  #[inline(always)]
  pub const fn require_leading(self) -> RequireLeading<Self> {
    RequireLeading::new(self)
  }

  define_many_delimited_methods!(Lang);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IsVariant)]
pub(super) enum State<T, S> {
  Start,
  Element,
  Leading(Spanned<T, S>),
  Separator(Spanned<T, S>),
}
