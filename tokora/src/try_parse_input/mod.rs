//! Tentative, backtracking parsing via [`TryParseInput`] and `ParseAttempt`.
//!
//! # Transactional contract (important)
//!
//! The backtracking guarantee of a declined attempt covers **valid tokens only**. When a
//! parser returns `Ok(ParseAttempt::Decline)`, the input position is restored so that no
//! *valid* tokens are consumed — but a decline is **not** fully transactional:
//!
//! - lexer-**ERROR** tokens encountered during the attempt may already have been consumed
//!   (the cursor advances past them), and
//! - any diagnostics **emitted** during the attempt are **left in place** — emissions are
//!   never rolled back.
//!
//! Only valid-token consumption is undone on decline; error-token consumption and emitted
//! errors persist. Callers and higher-level parsing layers that rely on backtracking must
//! account for this.

use derive_more::{IsVariant, TryUnwrap, Unwrap};

use crate::{
  input::InputRef,
  parser::{Accepted, ByRef, Fold, Repeated, Separated, TryFold, TryFoldWith},
  punct::*,
  token::PunctuatorToken,
};

#[cfg(any(feature = "alloc", feature = "std"))]
use crate::parser::RFold;

use super::*;

pub use ParseAttempt::{Accept, Decline};

/// Result type for tentative parsing attempts.
#[derive(Debug, Clone, PartialEq, Eq, IsVariant, TryUnwrap, Unwrap)]
#[unwrap(ref, ref_mut)]
#[try_unwrap(ref, ref_mut)]
pub enum ParseAttempt<O> {
  /// Parser successfully matched and produced a value.
  Accept(O),
  /// Parser declined to match without consuming any valid tokens.
  #[unwrap(ignore)]
  #[try_unwrap(ignore)]
  Decline,
}

impl<O> From<Option<O>> for ParseAttempt<O> {
  #[inline(always)]
  fn from(opt: Option<O>) -> Self {
    match opt {
      Some(value) => Self::Accept(value),
      None => Self::Decline,
    }
  }
}

impl<O> From<ParseAttempt<O>> for Option<O> {
  #[inline(always)]
  fn from(result: ParseAttempt<O>) -> Self {
    match result {
      ParseAttempt::Accept(value) => Some(value),
      ParseAttempt::Decline => None,
    }
  }
}

impl<O> ParseAttempt<O> {
  /// Converts to a `ParseAttempt` with a reference to the inner value.
  #[inline(always)]
  pub const fn as_ref(&self) -> ParseAttempt<&O> {
    match self {
      Self::Accept(value) => ParseAttempt::Accept(value),
      Self::Decline => ParseAttempt::Decline,
    }
  }

  /// Converts to a `ParseAttempt` with a mutable reference to the inner value.
  #[inline(always)]
  pub const fn as_mut(&mut self) -> ParseAttempt<&mut O> {
    match self {
      Self::Accept(value) => ParseAttempt::Accept(value),
      Self::Decline => ParseAttempt::Decline,
    }
  }

  /// Maps the inner value using the given function.
  #[inline(always)]
  pub fn map<U, F>(self, f: F) -> ParseAttempt<U>
  where
    F: FnOnce(O) -> U,
  {
    match self {
      Self::Accept(value) => ParseAttempt::Accept(f(value)),
      Self::Decline => ParseAttempt::Decline,
    }
  }

  /// Maps the inner value using the given fallible function.
  #[inline(always)]
  pub fn and_then<U, F, E>(self, f: F) -> Result<ParseAttempt<U>, E>
  where
    F: FnOnce(O) -> Result<U, E>,
  {
    match self {
      Self::Accept(value) => Ok(ParseAttempt::Accept(f(value)?)),
      Self::Decline => Ok(ParseAttempt::Decline),
    }
  }

  /// The named `Option` projection: `Accept(v)` → `Some(v)`, `Decline` → `None`.
  ///
  /// The `From<ParseAttempt<O>> for Option<O>` conversion already existed; nothing named
  /// it, so call sites reached for a `match` instead. This is that conversion with a name
  /// that can be found — and the composition point for the rest of `Option`'s vocabulary
  /// (`ok_or`, `unwrap_or_default`, `filter`, …), which is why no aliases for those ship
  /// beside it.
  #[inline(always)]
  pub fn into_option(self) -> Option<O> {
    match self {
      Self::Accept(value) => Some(value),
      Self::Decline => None,
    }
  }
}

/// Decline-propagation for hand-sequenced `try_*` productions — `?` for the three-way
/// vocabulary.
///
/// `let v = attempt!(try_x(inp));` applies `?` to the expression, unwraps an `Accept`, and
/// early-returns `Ok(ParseAttempt::Decline)` on a decline. Without it, a production that
/// sequences several tentative parses writes the same four-line `match` at every step, and
/// the enclosing function's decline arm is exactly where a hand-written one gets it wrong.
///
/// The enclosing function must return
/// `Result<ParseAttempt<_>, _>`; that is what the early return produces.
///
/// ```ignore
/// fn try_pair<'inp, Ctx>(inp: &mut InputRef<'inp, '_, L, Ctx>) -> Result<ParseAttempt<(A, B)>, E> {
///   let a = tokora::attempt!(try_a(inp));
///   let b = tokora::attempt!(try_b(inp));
///   Ok(ParseAttempt::Accept((a, b)))
/// }
/// ```
#[macro_export]
macro_rules! attempt {
  ($expr:expr) => {
    match $expr? {
      $crate::try_parse_input::ParseAttempt::Accept(value) => value,
      $crate::try_parse_input::ParseAttempt::Decline => {
        return ::core::result::Result::Ok($crate::try_parse_input::ParseAttempt::Decline);
      }
    }
  };
}

macro_rules! define_separated_by {
  ($($name:ident),+$(,)?) => {
    paste::paste! {
      $(
        #[doc = "Creates a `Separated` combinator which separates elements by the `" $name:snake "` separator and applies this parser repeatedly."]
        ///
        /// See [`separated`](crate::TryParseInput::separated) for details.
        #[inline(always)]
        fn [< separated_by_ $name:snake>](
          self,
        ) -> Separated<Self, $name<(), (), Lang>, O, L, Ctx, Lang, Cmpl>
        where
          Self: Sized,
          L: Lexer<'inp>,
          L::Token: PunctuatorToken<'inp>,
          Ctx: ParseContext<'inp, L, Lang>,
        {
          Separated::new(self)
        }
      )*
    }
  };
}

/// Tentative parsing trait for optional token consumption with automatic backtracking.
///
/// Unlike [`ParseInput`] which must produce a value or error, `TryParseInput` allows parsers
/// to inspect the input and decide whether to consume it based on lookahead. If the parser
/// returns `Ok(ParseAttempt::Decline)`, **no valid tokens are consumed** - the input position only advances
/// past any error tokens that were consumed by the emitter.
///
/// **IMPORTANT:** backtracking on `Ok(ParseAttempt::Decline)` is transactional over
/// **valid tokens only**. A decline may still have consumed lexer-ERROR tokens, and any
/// diagnostics emitted during the attempt are **not** rolled back.
pub trait TryParseInput<'inp, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  /// Attempts to parse `O` from the input without committing.
  ///
  /// **IMPORTANT:**
  ///
  /// Implementations **must** uphold this contract:
  /// - ✅ `Ok(ParseAttempt::Accept(value))` - Parser succeeded, tokens consumed, value produced
  /// - ✅ `Ok(ParseAttempt::Decline)` - Parser declined, **no valid tokens consumed**
  ///   - Backtracking restores the input position over **valid tokens only**
  ///   - Lexer-ERROR tokens may still have been consumed, and any emitted diagnostics are **not** rolled back
  /// - ✅ `Err(error)` - Parser encountered an error (may have consumed tokens)
  fn try_parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;

  /// Applies combinator on [`ParseAttempt::Accept`].
  #[inline(always)]
  fn accepted(self) -> Accepted<Self, L, O, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    Accepted::new(self)
  }

  /// Method form of the free [`opt`](crate::parser::opt) — a committed parser over
  /// `Option<O>`: `Accept(v)` → `Some(v)`, `Decline` → `None`.
  #[inline(always)]
  fn opt(
    self,
  ) -> impl for<'c> FnMut(
    &mut crate::input::InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
  ) -> crate::parser::OptOf<'inp, L, Ctx, Lang, O>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: crate::ComposableParseContext<'inp, L, Lang>,
    Cmpl: crate::input::Completeness,
  {
    crate::parser::opt(self)
  }

  /// Create a parser over a mutable reference to this parser.
  #[inline(always)]
  fn by_ref(&mut self) -> &mut ByRef<Self> {
    ByRef::from_ref_mut(self)
  }

  /// Creates a `Repeated` combinator that applies this parser repeatedly until it signals completion.
  ///
  /// The parser will be called repeatedly until:
  /// - It returns `Ok(ParseAttempt::Decline)` - parser peeked ahead, didn't match (no tokens consumed)
  /// - It returns `Err(e)` - parse error
  ///
  /// ## Key Behavior
  ///
  /// Since this parser implements [`TryParseInput`], when it returns `Ok(ParseAttempt::Decline)`:
  /// - The parser **peeked ahead** and saw tokens it doesn't match
  /// - **No tokens were consumed** - input position unchanged
  /// - Repetition **stops cleanly**
  ///
  /// ## When to Use This
  ///
  /// Use `.repeated()` when:
  /// - You want to parse zero or more occurrences
  /// - The parser can look ahead and decide if it matches
  /// - You want automatic stopping based on parser's lookahead
  ///
  /// ## See Also
  ///
  /// - [`repeated_while`](crate::ParseInput::repeated_while) - When you want to provide explicit stopping condition
  #[inline(always)]
  fn repeated(self) -> Repeated<Self, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    Repeated::new(self)
  }

  /// Creates a `Fold` combinator that accumulates results using the provided initializer and accumulator.
  ///
  /// See also [`try_fold`](TryParseInput::try_fold), [`fold_while`](crate::ParseInput::fold_while), [try_fold_while](crate::ParseInput::try_fold_while).
  #[inline(always)]
  fn fold<Init, Acc>(self, init: Init, acc: Acc) -> Fold<Self, Init, Acc, L, O, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Init: FnMut() -> O,
    Acc: FnMut(O, O) -> O,
  {
    Fold::new(self, init, acc)
  }

  /// Creates a `TryFold` combinator that accumulates results using the provided initializer and fallible accumulator.
  ///
  /// See also [`try_fold_with`](Self::try_fold_with), [`fold_while`](crate::ParseInput::fold_while), [try_fold_while](crate::ParseInput::try_fold_while).
  #[inline(always)]
  fn try_fold<Init, Acc>(
    self,
    init: Init,
    acc: Acc,
  ) -> TryFold<Self, Init, Acc, L, O, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Init: FnMut() -> O,
    Acc: FnMut(O, O) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  {
    TryFold::new(self, init, acc)
  }

  /// Creates a `TryFoldWith` combinator that accumulates results using the provided initializer,
  /// fallible accumulator, and parsing state.
  ///
  /// See also [`try_fold`](Self::try_fold), [`fold_while`](crate::ParseInput::fold_while), [try_fold_while](crate::ParseInput::try_fold_while).
  #[inline(always)]
  fn try_fold_with<Init, Acc>(
    self,
    init: Init,
    acc: Acc,
  ) -> TryFoldWith<Self, Init, Acc, L, O, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Init: FnMut() -> O,
    Acc: FnMut(
      O,
      O,
      ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
    ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  {
    TryFoldWith::new(self, init, acc)
  }

  /// Creates a `RFold` combinator that accumulates results in reverse order using the provided
  /// initializer and accumulator.
  ///
  /// This buffers all parsed outputs before folding them from right to left.
  /// Parsing stops when this parser returns `Ok(ParseAttempt::Decline)`.
  ///
  /// See also [`fold`](Self::fold).
  #[cfg(any(feature = "alloc", feature = "std"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
  #[inline(always)]
  fn rfold<Init, Acc>(self, init: Init, acc: Acc) -> RFold<Self, Init, Acc, L, O, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Init: FnMut() -> O,
    Acc: FnMut(O, O) -> O,
  {
    RFold::new(self, init, acc)
  }

  /// Creates a `Separated` combinator that parses separated elements, where **this parser
  /// handles the lookahead**.
  ///
  /// This parser (the element parser) will be called repeatedly to parse elements separated
  /// by the given separator. Since this parser implements [`TryParseInput`], it can peek ahead
  /// and return `Ok(ParseAttempt::Decline)` when it doesn't match, **without consuming any tokens**.
  ///
  /// ## Key Behavior
  ///
  /// The combinator stops when this element parser returns `Ok(ParseAttempt::Decline)`:
  /// - Parser **peeked ahead** and saw tokens it doesn't match
  /// - **No tokens consumed** - separator or closing delimiter left in input
  /// - Parsing **stops cleanly**
  ///
  /// ## When to Use This
  ///
  /// Use `.separated()` when:
  /// - Your element parser has built-in lookahead (implements `TryParseInput`)
  /// - You want the element parser to decide when to stop
  /// - The parser returns `Ok(ParseAttempt::Decline)` for non-matching tokens
  ///
  /// ## See Also
  ///
  /// - [`separated_while`](crate::ParseInput::separated_while) - When you want to provide the lookahead/stopping logic externally
  #[inline(always)]
  fn separated<Sep>(self) -> Separated<Self, Sep, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Sep: Punctuator<'inp, L, Lang>,
  {
    Separated::new(self)
  }

  define_separated_by!(
    Comma,
    Semicolon,
    Dot,
    Colon,
    Pipe,
    Ampersand,
    Hyphen,
    Underscore,
    DoubleColon,
    Arrow,
    FatArrow,
    Tilde,
    Slash,
    Backslash,
    Percent,
    Dollar,
    Hash,
    At,
  );
}

impl<'inp, L, F, O, Ctx, Lang: ?Sized, Cmpl> TryParseInput<'inp, L, O, Ctx, Lang, Cmpl> for F
where
  F: FnMut(
    &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn try_parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    (self)(input)
  }
}

impl<'inp, L, F, O, Ctx, Lang: ?Sized, Cmpl> TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for &mut ByRef<F>
where
  F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn try_parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    (**self).try_parse_input(input)
  }
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
