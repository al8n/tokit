//! Parse context trait and implementations.
//!
//! A parse context bundles the error emitter, the cache configuration, and the two budgets a
//! parse runs against — the recursion depth it may descend to and the number of items its lexer
//! may produce. This provides better type inference and a simpler API compared to configuring
//! them separately.

use core::marker::PhantomData;

use crate::{
  Cache, Emitter, Lexer,
  cache::DefaultCache,
  emitter::{ComposableEmitter, Fatal, FromTokenErrors, PolicyComposableEmitter},
  input::{InputContext, TokenBudget},
  lexer::SliceOf,
  state::recursion_tracker::RecursionLimiter,
};

/// A context that provides emitter and cache configuration for parsing.
pub trait ParseContext<'inp, L, Lang: ?Sized = ()> {
  /// The emitter type used for error handling.
  type Emitter: Emitter<'inp, L, Lang>
  where
    L: Lexer<'inp>;

  /// The cache type used for lookahead.
  type Cache: Cache<'inp, L, Lang>
  where
    L: Lexer<'inp>;

  /// Provides the emitter and cache instances for parsing, inside the
  /// [`InputContext`] that also carries the recursion budget the parse descends against and the
  /// [`TokenBudget`] its lexer produces items against.
  fn provide(self) -> InputContext<Self::Emitter, Self::Cache>
  where
    L: Lexer<'inp>;
}

impl<'inp, L, Lang: ?Sized> ParseContext<'inp, L, Lang> for ()
where
  L: Lexer<'inp>,
  Fatal<(), Lang>: Emitter<'inp, L, Lang>,
{
  type Emitter = Fatal<(), Lang>;
  type Cache = DefaultCache<'inp, L>;

  #[inline(always)]
  fn provide(self) -> InputContext<Self::Emitter, Self::Cache>
  where
    L: Lexer<'inp>,
  {
    InputContext::new(Fatal::of(), DefaultCache::<'inp, L>::new())
  }
}

/// Custom context: use a custom emitter and cache pair.
impl<'inp, L, E, C, Lang: ?Sized> ParseContext<'inp, L, Lang> for (E, C)
where
  L: Lexer<'inp>,
  E: Emitter<'inp, L, Lang>,
  C: Cache<'inp, L, Lang>,
{
  type Emitter = E;
  type Cache = C;

  fn provide(self) -> InputContext<Self::Emitter, Self::Cache>
  where
    L: Lexer<'inp>,
  {
    InputContext::new(self.0, self.1)
  }
}

/// Convenient type alias for the default parse context.
pub type FatalContext<'inp, L, Error, Lang = ()> =
  ParserContext<'inp, L, Fatal<Error, Lang>, DefaultCache<'inp, L>, Lang>;

/// A concrete [`ParseContext`] implementation that holds an emitter and optional cache options.
pub struct ParserContext<'inp, L, E, C = DefaultCache<'inp, L>, Lang: ?Sized = ()>
where
  L: Lexer<'inp>,
  E: Emitter<'inp, L, Lang>,
  C: Cache<'inp, L, Lang>,
{
  emitter: E,
  cache: Option<C::Options>,
  /// The recursion budget handed to the [`Input`](crate::input::Input) at
  /// [`provide`](ParseContext::provide) —
  /// [`RecursionLimiter::PARSE_DEFAULT_DEPTH`] unless
  /// [`with_recursion_limiter`](Self::with_recursion_limiter) says otherwise.
  recursion: RecursionLimiter,
  /// The token budget handed to the [`Input`](crate::input::Input) at
  /// [`provide`](ParseContext::provide) — [`TokenBudget::unlimited`] unless
  /// [`with_token_budget`](Self::with_token_budget) says otherwise.
  token_budget: TokenBudget,
  _marker: PhantomData<&'inp L>,
  _lang: PhantomData<Lang>,
}

impl<'inp, L, E, C> ParserContext<'inp, L, E, C>
where
  L: Lexer<'inp>,
  E: Emitter<'inp, L>,
  C: Cache<'inp, L>,
{
  /// Creates a new parser context with the given emitter.
  #[inline(always)]
  pub const fn new(emitter: E) -> Self {
    Self::of(emitter)
  }

  /// Creates a new parser context with the given emitter and cache options.
  #[inline(always)]
  pub const fn with_cache_options(emitter: E, options: C::Options) -> Self {
    Self::with_cache_options_of(emitter, options)
  }
}

impl<'inp, L, E, C, Lang: ?Sized> ParserContext<'inp, L, E, C, Lang>
where
  L: Lexer<'inp>,
  E: Emitter<'inp, L, Lang>,
  C: Cache<'inp, L, Lang>,
{
  const fn new_in(emitter: E, opts: Option<C::Options>) -> Self {
    Self {
      emitter,
      cache: opts,
      recursion: RecursionLimiter::with_limitation(RecursionLimiter::PARSE_DEFAULT_DEPTH),
      token_budget: TokenBudget::unlimited(),
      _marker: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Sets the **recursion budget** every parse driven from this context descends against.
  ///
  /// Threaded straight to
  /// [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter)
  /// by [`provide`](ParseContext::provide); see it for what the budget covers. The default is
  /// [`RecursionLimiter::PARSE_DEFAULT_DEPTH`], protection on — requested explicitly, not
  /// [`RecursionLimiter::new`]'s own (unrelated) general-purpose default of 500. **Read that
  /// constant rather than assuming a number.**
  ///
  /// ```rust,ignore
  /// // Deep but bounded grammar: raise the ceiling for this parse only.
  /// let ctx = ParserContext::of(Fatal::of())
  ///   .with_recursion_limiter(RecursionLimiter::with_limitation(4_000));
  /// let value = Parser::with_context(ctx).apply(expr).parse_str(src)?;
  /// ```
  #[inline(always)]
  pub const fn with_recursion_limiter(mut self, recursion: RecursionLimiter) -> Self {
    self.recursion = recursion;
    self
  }

  /// Sets the **token budget** every parse driven from this context produces items against.
  ///
  /// Threaded straight to [`InputContext::with_token_budget`](crate::input::InputContext::with_token_budget)
  /// by [`provide`](ParseContext::provide); see it, and [`TokenBudget`], for what the ceiling
  /// counts. The default is [`TokenBudget::unlimited`] — protection **off**, unlike the recursion
  /// budget beside it — so nothing changes for a parse that does not ask.
  ///
  /// ```rust,ignore
  /// // Untrusted input: refuse past a million lexed items, tokens and errors alike.
  /// let ctx = ParserContext::of(Fatal::of())
  ///   .with_token_budget(TokenBudget::with_limitation(1_000_000));
  /// ```
  #[inline(always)]
  pub const fn with_token_budget(mut self, token_budget: TokenBudget) -> Self {
    self.token_budget = token_budget;
    self
  }

  /// Creates a new parser context with the given emitter for a specific language.
  #[inline(always)]
  pub const fn of(emitter: E) -> Self {
    Self::new_in(emitter, None)
  }

  /// Creates a new parser context with the given emitter and cache options for a specific language.
  #[inline(always)]
  pub const fn with_cache_options_of(emitter: E, options: C::Options) -> Self {
    Self::new_in(emitter, Some(options))
  }
}

impl<'inp, L, E, C, Lang: ?Sized> ParseContext<'inp, L, Lang> for ParserContext<'inp, L, E, C, Lang>
where
  L: Lexer<'inp>,
  E: Emitter<'inp, L, Lang>,
  C: Cache<'inp, L, Lang>,
{
  type Emitter = E;
  type Cache = C;

  #[inline(always)]
  fn provide(self) -> InputContext<Self::Emitter, Self::Cache>
  where
    L: Lexer<'inp>,
  {
    match self.cache {
      Some(options) => InputContext::new(self.emitter, C::with_options(options)),
      None => InputContext::new(self.emitter, C::new()),
    }
    .with_recursion_limiter(self.recursion)
    .with_token_budget(self.token_budget)
  }
}

/// The error type context `Ctx`'s emitter produces.
///
/// Generic parser signatures name this projection in every return type, and spelling it
/// out means chaining [`ParseContext::Emitter`] through [`Emitter::Error`]. `ErrorOf`
/// names that path once, so `Result<T, ErrorOf<'inp, L, Ctx, Lang>>` stays legible.
///
/// # Examples
///
/// The alias is definitionally the nested projection — this identity function compiles
/// precisely because the two spellings are the same type:
///
/// ```rust
/// use tokora::{Emitter, ErrorOf, Lexer, ParseContext};
///
/// fn same_type<'inp, L, Ctx>(
///   err: <Ctx::Emitter as Emitter<'inp, L>>::Error,
/// ) -> ErrorOf<'inp, L, Ctx, ()>
/// where
///   L: Lexer<'inp>,
///   Ctx: ParseContext<'inp, L>,
/// {
///   err
/// }
/// ```
pub type ErrorOf<'inp, L, Ctx, Lang> =
  <<Ctx as ParseContext<'inp, L, Lang>>::Emitter as Emitter<'inp, L, Lang>>::Error;

/// The one bound a generic parser atom needs, at the **default policy**.
///
/// It carries [`ComposableEmitter`](crate::emitter::ComposableEmitter), which is the
/// collecting combinators' unpolicied surface. A production that attaches a count or
/// separator policy — `at_most`, `bounded`, `require_leading`, `require_trailing` — needs
/// [`PolicyParseContext`] instead, which is this bound plus the three policy emitters. Both
/// tiers are one bound; the wider one is not the default because it would push
/// `From<TooMany>` and `From<MissingToken>` onto every consumer's error type whether or not a
/// policy is ever used.
///
/// A grammar function has two orthogonal requirements of its context: that the emitter can
/// *route* every diagnostic, and that the error type can *absorb* one. `ComposableParseContext`
/// carries both. It is implemented for every [`ParseContext`] whose emitter is a
/// [`ComposableEmitter`](crate::emitter::ComposableEmitter), whose emitter's error absorbs
/// every token-level failure ([`FromTokenErrors`](crate::emitter::FromTokenErrors)), and
/// whose source slice is [`Clone`] — so an atom needs only
/// `Ctx: ComposableParseContext<'inp, L>` and restates nothing.
///
/// # Mechanism, and why it is a nested associated-type bound
///
/// Both requirements ride the [`ParseContext`] supertrait as *nested* associated-type
/// bounds — `Emitter: ComposableEmitter<…, Error: FromTokenErrors<…>>` — so they elaborate
/// to callers of the bundle. A free-standing `where ErrorOf<'inp, L, Ctx, Lang>: …` clause
/// does **not** work: rustc will not elaborate a where-clause predicate whose self type is
/// a projection, so the obligation would reappear at every use site. The
/// `SliceOf<'inp, L>: Clone` requirement lives on the blanket impl instead (a projection
/// bound cannot be a supertrait), so it gates which contexts qualify without forcing that
/// clause onto every mention of the bound; atoms that clone a slice restate that one bound
/// locally.
///
/// # What this means for an implementor
///
/// Nothing is written: the trait is blanket-implemented, and a context qualifies exactly
/// when its emitter's error absorbs the five token-level conversions. A context whose error
/// cannot absorb an end-of-input — which was never able to run a real grammar — now fails
/// at the bound instead of hundreds of frames deep at the first leaf atom that tries.
///
/// # Examples
///
/// One bound stands in for the emitter ladder *and* the error conversions — the bundled
/// function can call into code demanding either:
///
/// ```rust
/// use tokora::{ComposableParseContext, ErrorOf, Lexer, Token};
/// use tokora::emitter::{
///   FromUnclosed, SeparatedEmitter, TooFewEmitter, UnclosedEmitter,
///   UnexpectedTrailingSeparatorEmitter,
/// };
/// use tokora::error::{UnexpectedEot, token::UnexpectedTokenOf};
///
/// fn needs_diagnostics<'inp, L, E>()
/// where
///   L: Lexer<'inp>,
///   E: SeparatedEmitter<'inp, L>
///     + TooFewEmitter<'inp, L>
///     + UnexpectedTrailingSeparatorEmitter<'inp, L>
///     + UnclosedEmitter<'inp, L>,
/// {
/// }
///
/// fn needs_conversions<'inp, L, E>()
/// where
///   L: Lexer<'inp>,
///   E: From<UnexpectedEot<L::Offset, ()>>
///     + From<UnexpectedEot<L::Offset, (), <L::Token as Token<'inp>>::Kind>>
///     + From<UnexpectedTokenOf<'inp, L>>
///     + From<<L::Token as Token<'inp>>::Error>
///     + FromUnclosed<'inp, L>,
/// {
/// }
///
/// // The single bound elaborates to both halves.
/// fn atom_shaped<'inp, L, Ctx>()
/// where
///   L: Lexer<'inp>,
///   Ctx: ComposableParseContext<'inp, L>,
/// {
///   needs_diagnostics::<L, Ctx::Emitter>();
///   needs_conversions::<L, ErrorOf<'inp, L, Ctx, ()>>();
/// }
/// ```
pub trait ComposableParseContext<'inp, L, Lang: ?Sized = ()>:
  ParseContext<
    'inp,
    L,
    Lang,
    Emitter: ComposableEmitter<'inp, L, Lang, Error: FromTokenErrors<'inp, L, Lang>>,
  >
where
  L: Lexer<'inp>,
{
}

impl<'inp, L, Lang: ?Sized, T> ComposableParseContext<'inp, L, Lang> for T
where
  L: Lexer<'inp>,
  SliceOf<'inp, L>: Clone,
  T: ParseContext<
      'inp,
      L,
      Lang,
      Emitter: ComposableEmitter<'inp, L, Lang, Error: FromTokenErrors<'inp, L, Lang>>,
    >,
{
}

/// [`ComposableParseContext`] widened to the count- and separator-**policy** builders.
///
/// Where the default bundle covers `separated_by_*` / `delimited` / `repeated` with no policy
/// attached, this one additionally elaborates [`TooManyEmitter`](crate::emitter::TooManyEmitter),
/// [`MissingLeadingSeparatorEmitter`](crate::emitter::MissingLeadingSeparatorEmitter) and
/// [`MissingTrailingSeparatorEmitter`](crate::emitter::MissingTrailingSeparatorEmitter), so
/// `.at_most(n)`, `.bounded(a, b)`, `.require_leading()` and `.require_trailing()` drive under
/// one bound.
///
/// The name says what the delta *is*. "Full" would be falsified the day someone reads it: the
/// pratt engine is in neither bundle, and names its own emitter and conversions.
///
/// Same mechanism as its narrower sibling — a nested associated-type bound on the
/// [`ParseContext`] supertrait, so both halves elaborate to callers — and the same
/// nothing-to-write story for implementors: it is blanket-implemented.
pub trait PolicyParseContext<'inp, L, Lang: ?Sized = ()>:
  ComposableParseContext<'inp, L, Lang>
  + ParseContext<
    'inp,
    L,
    Lang,
    Emitter: PolicyComposableEmitter<'inp, L, Lang, Error: FromTokenErrors<'inp, L, Lang>>,
  >
where
  L: Lexer<'inp>,
{
}

impl<'inp, L, Lang: ?Sized, T> PolicyParseContext<'inp, L, Lang> for T
where
  L: Lexer<'inp>,
  SliceOf<'inp, L>: Clone,
  T: ComposableParseContext<'inp, L, Lang>
    + ParseContext<
      'inp,
      L,
      Lang,
      Emitter: PolicyComposableEmitter<'inp, L, Lang, Error: FromTokenErrors<'inp, L, Lang>>,
    >,
{
}

#[cfg(test)]
mod tests;
