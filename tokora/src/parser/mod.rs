//! Blazing fast parser combinators with deterministic parsing and zero-copy streaming.
//!
//! This module provides a unique parser combinator framework that combines:
//!
//! 1. **Parse-While-Lexing Architecture**: Zero-copy streaming - tokens consumed directly from
//!    the lexer without buffering, eliminating allocation overhead
//! 2. **Deterministic LALR-Style Parsing**: Explicit lookahead with compile-time buffer capacity, no hidden backtracking
//! 3. **Flexible Error Handling**: Same parser adapts for fail-fast runtime ([`Fatal`](crate::emitter::Fatal))
//!    or greedy compiler diagnostics (via custom [`Emitter`](crate::Emitter))
//!
//! # Architecture
//!
//! Unlike traditional parser combinators that buffer all tokens and rely on implicit backtracking:
//!
//! **Traditional (Two-Phase)**:
//! ```text
//! Source → Lexer → [Vec<Token>] → Parser
//!                   ↑ Extra allocation!
//! ```
//!
//! **Tokora (Streaming)**:
//! ```text
//! Source → Lexer ←→ Parser
//!          ↑________↓
//!     Zero-copy, on-demand
//! ```
//!
//! Parsers pull tokens on-demand from the lexer. Only a small lookahead window (1-32 tokens)
//! is buffered on the stack for deterministic decisions.
//!
//! # Core Concepts
//!
//! ## Parse-While-Lexing
//!
//! Tokens flow directly from lexer to parser without intermediate buffering:
//! - **Zero extra allocations**: No `Vec<Token>` buffer
//! - **Lower memory**: Only lookahead window buffered on stack
//! - **Better cache locality**: Tokens processed immediately after lexing
//!
//! ## Deterministic Parsing (No Hidden Backtracking)
//!
//! Unlike traditional parser combinators with implicit backtracking, Tokora uses
//! **explicit lookahead-based decisions**:
//!
//! ```ignore
//! // Traditional: Hidden backtracking
//! let parser = try_parser1.or(try_parser2).or(try_parser3);
//!
//! // Tokora: Explicit lookahead, deterministic
//! let parser = any().peek_then::<_, typenum::U2>(|peeked, _| {
//!     match peeked.front() {
//!         Some(Token::If) => Ok(Action::Continue),  // Deterministic!
//!         _ => Ok(Action::Stop),
//!     }
//! });
//! ```
//!
//! The [`Window`] trait provides compile-time fixed lookahead capacity (`typenum::U1` to `typenum::U32`),
//! enabling LALR-style deterministic table parsing.
//!
//! ## Flexible Error Handling via Emitter
//!
//! The [`Emitter`](crate::Emitter) trait decouples parsing logic from error handling strategy:
//!
//! ```ignore
//! // Fail-fast for runtime/REPL (stop on first error)
//! let parser = Parser::with_context(FatalContext::new());
//! let result = parser.parse(source);  // Uses Fatal emitter
//!
//! // Custom greedy emitter for compiler diagnostics (collect all errors)
//! struct DiagnosticEmitter { errors: Vec<Error> }
//! impl Emitter for DiagnosticEmitter { /* collect errors */ }
//! ```
//!
//! **Same parser code, different behavior** - just swap the `Emitter` type.
//!
//! # Quick Start
//!
//! ```ignore
//! use tokora::{Any, Parse, Parser, parser::FatalContext};
//!
//! // 1. Parse any token
//! let parser = Any::parser::<'_, MyLexer<'_>, ()>();
//! let result = parser.parse(source);
//!
//! // 2. Chain combinators
//! let parser = Any::parser::<'_, MyLexer<'_>, ()>()
//!     .map(|tok| tok.kind())
//!     .filter(|kind| matches!(kind, TokenKind::Number));
//!
//! // 3. Explicit lookahead (deterministic choice)
//! let parser = Any::parser::<'_, MyLexer<'_>, ()>()
//!     .peek_then::<_, typenum::U1>(|peeked, _| {
//!         match peeked.get(0) {
//!             Some(tok) if tok.is_keyword("if") => Ok(Action::Continue),
//!             _ => Ok(Action::Stop),
//!         }
//!     });
//! ```
//!
//! # Available Combinators
//!
//! ## Basic Parsers
//!
//! - `any` - Accept any single token
//! - `expect` - Expect specific token, emit error if not found
//! - `empty` - No-op parser
//! - `todo` - Placeholder for incomplete implementations
//!
//! ## Sequencing
//!
//! - `then` - Sequential composition: parse `p1` then `p2`
//! - `then_ignore` - Parse both, keep only first result
//! - `ignore_then` - Parse both, keep only second result
//!
//! ## Repetition & Collections
//!
//! - `repeated` - Repeat until condition returns `Action::Stop`
//! - `separated_by` - Parse elements separated by delimiter
//! - `delim` - Parse delimited content (e.g., parentheses)
//! - `delim_seq` - Parse delimited, separated sequences
//! - `delimited`/`parens`/`braces`/`brackets`/`angles` (+ `try_` attempt twins) - one delimited region as a span-carrying `Delimited`
//!
//! ## Lookahead & Conditional (Deterministic)
//!
//! - `peek_then` - Peek ahead with fixed window, make deterministic decision
//! - `peek_then_choice` - Choose between alternatives based on lookahead
//!
//! ## Transformation
//!
//! - `map` - Transform output
//! - `filter` - Filter with validation
//! - `filter_map` - Filter and transform
//! - `validate` - Validate with full location context
//!
//! ## Error Recovery
//!
//! - `recover` - Try parser, use recovery on error with backtracking
//! - `inplace_recover` - Try parser, use recovery on error without backtracking
//! - `padded` - Skip trivia (whitespace/comments) before and after
//!
//! # Performance Characteristics
//!
//! - **Memory**: O(1) - only small lookahead window on stack, no token buffering
//! - **Parsing**: O(n) - single-pass, deterministic, no backtracking
//! - **Lookahead**: O(1) - fixed compile-time capacity (1-32 tokens)
//!
//! # Design Priorities
//!
//! 1. **Performance**: Parse-while-lexing (zero-copy), no hidden allocations
//! 2. **Predictability**: No hidden backtracking, deterministic decisions
//! 3. **Composability**: Small parsers combine into complex grammars
//! 4. **Versatility**: Same parser for runtime (fail-fast) or compiler (greedy) via `Emitter`

#![allow(clippy::type_complexity)]

use core::marker::PhantomData;

use crate::{
  Emitter, Lexer, Source, Token,
  cache::Peeked,
  emitter::{Fatal, FromEmitterError},
  error::{UnexpectedEot, token::UnexpectedToken},
  input::{Complete, Completeness, Input, InputRef, SurfaceIncomplete},
  located::Located,
  parse_context::{FatalContext, ParseContext},
  parse_input::*,
  parse_state::ParseState,
  slice::Sliced,
  span::Spanned,
  utils::{
    Expected,
    marker::{PhantomLocated, PhantomSliced, PhantomSpan},
  },
};

use derive_more::{IsVariant, TryUnwrap, Unwrap};

pub use accepted::*;
pub use any::*;
pub use by_ref::*;
pub use collect::Collect;
pub use delimited::*;
pub use empty::*;
pub use expect::*;
pub use fail::*;
pub use filter::*;
pub use filter_map::*;
pub use fold::*;
pub use ident_list::*;
pub use ignore::*;
pub use labelled::*;
#[cfg(any(feature = "alloc", feature = "std"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
pub use list::*;
pub use many::*;
pub use map::*;
pub use node::*;
pub use opt::*;
pub use padded::*;
pub use peek::*;
pub use pratt::*;
pub use recover::*;
pub use skip_then_retry::*;
pub use then::*;
pub use todo::*;
pub use unwrapped::*;
pub use validate::*;
pub use with::*;

mod accepted;
mod any;
mod by_ref;
mod collect;
mod delimited;
mod empty;
mod expect;
mod fail;
mod filter;
mod filter_map;
mod fold;
mod ident;
mod ident_list;
mod ignore;
mod keyword;
mod labelled;
#[cfg(any(feature = "alloc", feature = "std"))]
mod list;
mod many;
mod map;
mod node;
mod opt;
mod padded;
mod peek;
mod pratt;
mod punct;
mod recover;
mod skip_then_retry;
mod then;
mod todo;
mod unwrapped;
mod validate;
mod with;

/// Wrapper for cache configuration in parsers.
///
/// Wraps a cache type `C` to distinguish it from bare `()` in type parameters,
/// preventing trait overlap in Parse implementations.
#[repr(transparent)]
pub struct WithCache<'inp, L, C> {
  cache: C,
  _marker: PhantomData<&'inp L>,
}

/// Wrapper for emitter configuration in parsers.
///
/// Wraps an emitter type `E` to distinguish it from bare `()` in type parameters,
/// preventing trait overlap in Parse implementations.
#[repr(transparent)]
pub struct WithEmitter<E: ?Sized>(E);

/// A parser with configurable emitter and cache.
///
/// # Type Parameters
///
/// - `F`: The parsing function
/// - `L`: The lexer type
/// - `O`: The output type
/// - `Error`: The error type
/// - `Options`: Configuration for emitter and cache (defaults to `ParserOptions<L>`)
///
/// # Examples
///
/// ```ignore
/// // Create parser with defaults
/// let p = Parser::with(|inp| inp.next());
///
/// // Configure emitter
/// let p = Parser::with(|inp| inp.next())
///     .with_emitter(MyEmitter::new());
/// ```
pub struct Parser<F, L: ?Sized, O: ?Sized, Context, Error: ?Sized> {
  f: F,
  ctx: Context,
  _l: PhantomData<L>,
  _o: PhantomData<O>,
  _e: PhantomData<Error>,
}

impl<F, L, O, Context, Error> core::ops::Deref for Parser<F, L, O, Context, Error> {
  type Target = F;

  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.f
  }
}

impl<F, L, O, Context, Error> core::ops::DerefMut for Parser<F, L, O, Context, Error> {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.f
  }
}

impl<'inp, L, O, Error> Default for Parser<(), L, O, FatalContext<'inp, L, Error>, Error>
where
  L: Lexer<'inp>,
  Error: FromEmitterError<'inp, L>,
{
  #[inline(always)]
  fn default() -> Self {
    Parser::new()
  }
}

impl Parser<(), (), (), (), ()> {
  /// A parser without any behavior.
  #[inline(always)]
  pub const fn new<'inp, L, O, Error>() -> Parser<(), L, O, FatalContext<'inp, L, Error>, Error>
  where
    L: Lexer<'inp>,
    Error: FromEmitterError<'inp, L>,
  {
    Self::of()
  }

  /// Creates a parser with the given context.
  #[inline(always)]
  pub const fn with_context<'inp, L, O, Ctx, Error>(ctx: Ctx) -> Parser<(), L, O, Ctx, Error>
  where
    L: Lexer<'inp>,
    Error: FromEmitterError<'inp, L>,
    Ctx: ParseContext<'inp, L>,
    Ctx::Emitter: Emitter<'inp, L, Error = Error>,
  {
    Self::with_context_of(ctx)
  }

  /// A parser without any behavior.
  #[inline(always)]
  pub const fn of<'inp, L, O, Error, Lang>()
  -> Parser<(), L, O, FatalContext<'inp, L, Error, Lang>, Error>
  where
    L: Lexer<'inp>,
    Error: FromEmitterError<'inp, L, Lang>,
    Lang: ?Sized,
  {
    Self::with_context_of(FatalContext::of(Fatal::of()))
  }

  /// Creates a parser with the given context for a specific language.
  #[inline(always)]
  pub const fn with_context_of<'inp, L, O, Error, Ctx, Lang>(
    ctx: Ctx,
  ) -> Parser<(), L, O, Ctx, Error>
  where
    L: Lexer<'inp>,
    Error: FromEmitterError<'inp, L, Lang>,
    Ctx: ParseContext<'inp, L, Lang>,
    Ctx::Emitter: Emitter<'inp, L, Lang, Error = Error>,
    Lang: ?Sized,
  {
    Parser {
      f: (),
      ctx,
      _l: PhantomData,
      _o: PhantomData,
      _e: PhantomData,
    }
  }

  /// Creates a parser with a parser function and the fatal context.
  #[inline(always)]
  pub const fn with_parser<'inp, L, O, Error, F>(
    f: F,
  ) -> Parser<F, L, O, FatalContext<'inp, L, Error>, Error>
  where
    L: Lexer<'inp>,
    F: ParseInput<'inp, L, O, FatalContext<'inp, L, Error>>,
    Error: FromEmitterError<'inp, L>,
  {
    Self::with_parser_of(f)
  }

  /// Creates a parser with a parser function and the fatal context for a specific language.
  #[inline(always)]
  pub const fn with_parser_of<'inp, L, O, Error, F, Lang>(
    f: F,
  ) -> Parser<F, L, O, FatalContext<'inp, L, Error, Lang>, Error>
  where
    L: Lexer<'inp>,
    F: ParseInput<'inp, L, O, FatalContext<'inp, L, Error, Lang>, Lang>,
    Error: FromEmitterError<'inp, L, Lang>,
    Lang: ?Sized,
  {
    Self::with_parser_and_context_of(f, FatalContext::of(Fatal::of()))
  }

  /// Creates a parser with a parser function and the fatal context.
  #[inline(always)]
  pub const fn with_parser_and_context<'inp, L, O, Error, Ctx, F>(
    f: F,
    ctx: Ctx,
  ) -> Parser<F, L, O, Ctx, Error>
  where
    L: Lexer<'inp>,
    F: ParseInput<'inp, L, O, Ctx>,
    Ctx: ParseContext<'inp, L>,
    Error: FromEmitterError<'inp, L>,
  {
    Self::with_parser_and_context_of(f, ctx)
  }

  /// Creates a parser with a parser function and the fatal context for a specific language.
  #[inline(always)]
  pub const fn with_parser_and_context_of<'inp, L, O, Error, Ctx, F, Lang>(
    f: F,
    ctx: Ctx,
  ) -> Parser<F, L, O, Ctx, Error>
  where
    L: Lexer<'inp>,
    F: ParseInput<'inp, L, O, Ctx, Lang>,
    Ctx: ParseContext<'inp, L, Lang>,
    Error: FromEmitterError<'inp, L, Lang>,
    Lang: ?Sized,
  {
    Parser {
      f,
      ctx,
      _l: PhantomData,
      _o: PhantomData,
      _e: PhantomData,
    }
  }
}

impl<'inp, L, O, Ctx, Error> Parser<(), L, O, Ctx, Error>
where
  L: Lexer<'inp>,
{
  /// Apply a new parsing function to the parser.
  #[inline(always)]
  pub fn apply<F>(self, f: F) -> Parser<F, L, O, Ctx, Error>
  where
    Ctx: ParseContext<'inp, L>,
    F: ParseInput<'inp, L, O, Ctx>,
  {
    self.apply_of(f)
  }

  /// Apply a new parsing function to the parser for a specific language.
  #[inline(always)]
  pub fn apply_of<F, Lang>(self, f: F) -> Parser<F, L, O, Ctx, Error>
  where
    Ctx: ParseContext<'inp, L, Lang>,
    F: ParseInput<'inp, L, O, Ctx, Lang>,
  {
    Parser {
      f,
      ctx: self.ctx,
      _l: PhantomData,
      _o: PhantomData,
      _e: PhantomData,
    }
  }
}

/// Entry-point trait: run a parser against a source.
///
/// This provides the ergonomic `.parse()` API similar to Chumsky and
/// Winnow. Implementations wire up `Input`, `Emitter`, and `Cache`
/// before delegating to [`ParseInput`].
pub trait Parse<'inp, L, O, Error, Lang: ?Sized = ()>: Sized {
  /// Parse using the lexer's default state.
  #[inline(always)]
  fn parse(self, src: &'inp L::Source) -> Result<O, Error>
  where
    L: Lexer<'inp>,
    L::State: Default,
  {
    self.parse_with_state(src, L::State::default())
  }

  /// Parse using an explicit lexer state.
  fn parse_with_state(self, src: &'inp L::Source, state: L::State) -> Result<O, Error>
  where
    L: Lexer<'inp>;

  /// Parse from a raw string source.
  #[inline(always)]
  fn parse_str(self, src: &'inp str) -> Result<O, Error>
  where
    L: Lexer<'inp, Source = str>,
    L::State: Default,
  {
    self.parse_str_with_state(src, Default::default())
  }

  /// Parse from a raw string source with an explicit lexer state.
  #[inline(always)]
  fn parse_str_with_state(self, src: &'inp str, state: L::State) -> Result<O, Error>
  where
    L: Lexer<'inp, Source = str>,
  {
    self.parse_with_state(src, state)
  }

  /// Parse from a raw byte slice source.
  #[inline(always)]
  fn parse_slice(self, src: &'inp [u8]) -> Result<O, Error>
  where
    L: Lexer<'inp, Source = [u8]>,
    L::State: Default,
  {
    self.parse_slice_with_state(src, Default::default())
  }

  /// Parse from a raw byte slice source with an explicit lexer state.
  #[inline(always)]
  fn parse_slice_with_state(self, src: &'inp [u8], state: L::State) -> Result<O, Error>
  where
    L: Lexer<'inp, Source = [u8]>,
  {
    self.parse_with_state(src, state)
  }

  /// Parse from [`bytes::Bytes`](https://docs.rs/bytes/latest/bytes/struct.Bytes.html) source.
  #[cfg(feature = "bytes_1")]
  #[cfg_attr(docsrs, doc(cfg(feature = "bytes_1")))]
  #[inline(always)]
  fn parse_bytes(self, src: &'inp bytes_1::Bytes) -> Result<O, Error>
  where
    L: Lexer<'inp, Source = [u8]>,
    L::State: Default,
  {
    self.parse_bytes_with_state(src, Default::default())
  }

  /// Parse from [`bytes::Bytes`](https://docs.rs/bytes/latest/bytes/struct.Bytes.html) source with an explicit lexer state.
  #[cfg(feature = "bytes_1")]
  #[cfg_attr(docsrs, doc(cfg(feature = "bytes_1")))]
  #[inline(always)]
  fn parse_bytes_with_state(self, src: &'inp bytes_1::Bytes, state: L::State) -> Result<O, Error>
  where
    L: Lexer<'inp, Source = [u8]>,
  {
    self.parse_with_state(src.as_ref(), state)
  }

  /// Parse from [`bstr::BStr`](https://docs.rs/bstr/latest/bstr/struct.BStr.html) source.
  #[cfg(feature = "bstr_1")]
  #[cfg_attr(docsrs, doc(cfg(feature = "bstr_1")))]
  #[inline(always)]
  fn parse_bstr(self, src: &'inp bstr_1::BStr) -> Result<O, Error>
  where
    L: Lexer<'inp, Source = [u8]>,
    L::State: Default,
  {
    self.parse_bstr_with_state(src, Default::default())
  }

  /// Parse from [`bstr::BStr`](https://docs.rs/bstr/latest/bstr/struct.BStr.html) source with an explicit lexer state.
  #[cfg(feature = "bstr_1")]
  #[cfg_attr(docsrs, doc(cfg(feature = "bstr_1")))]
  #[inline(always)]
  fn parse_bstr_with_state(self, src: &'inp bstr_1::BStr, state: L::State) -> Result<O, Error>
  where
    L: Lexer<'inp, Source = [u8]>,
  {
    self.parse_with_state(src.as_ref(), state)
  }

  /// Parse from [`hipstr::HipStr`](https://docs.rs/hipstr/latest/hipstr/type.HipStr.html) source.
  #[cfg(feature = "hipstr_0_8")]
  #[cfg_attr(docsrs, doc(cfg(feature = "hipstr_0_8")))]
  #[inline(always)]
  fn parse_hipstr(self, src: &'inp hipstr_0_8::HipStr<'_>) -> Result<O, Error>
  where
    L: Lexer<'inp, Source = str>,
    L::State: Default,
  {
    self.parse_hipstr_with_state(src, Default::default())
  }

  /// Parse from [`hipstr::HipStr`](https://docs.rs/hipstr/latest/hipstr/type.HipStr.html) source with an explicit lexer state.
  #[cfg(feature = "hipstr_0_8")]
  #[cfg_attr(docsrs, doc(cfg(feature = "hipstr_0_8")))]
  #[inline(always)]
  fn parse_hipstr_with_state(
    self,
    src: &'inp hipstr_0_8::HipStr<'_>,
    state: L::State,
  ) -> Result<O, Error>
  where
    L: Lexer<'inp, Source = str>,
  {
    self.parse_with_state(src.as_str(), state)
  }
}

impl<'inp, F, L, O, Error, Ctx, Lang: ?Sized> Parse<'inp, L, O, Error, Lang>
  for Parser<F, L, O, Ctx, Error>
where
  F: ParseInput<'inp, L, O, Ctx, Lang>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: Emitter<'inp, L, Lang, Error = Error>,
{
  #[inline(always)]
  fn parse_with_state(self, src: &'inp L::Source, state: L::State) -> Result<O, Error> {
    let Parser { mut f, ctx, .. } = self;

    let (mut emitter, cache) = ctx.provide().into_components();
    let mut input = Input::with_state_and_cache(src, state, cache);
    let mut input_ref = input.as_ref(&mut emitter);
    f.parse_input(&mut input_ref)
  }
}

/// Type-level function for configuration transformations.
///
/// This trait enables progressive parser configuration by transforming
/// one configuration type into another. For example:
///
/// - `()` → `WithEmitter<E>` (add emitter configuration)
/// - `()` → `WithCache<C>` (add cache configuration)
///
/// Used internally by `.with_emitter()` and `.with_cache()` methods.
pub trait Apply<State> {
  /// The input required to perform the transformation
  type Options;

  /// Transform `self` into `State` using the provided `options`.
  fn apply(self, options: Self::Options) -> State;
}

/// A hint used during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IsVariant, Unwrap, TryUnwrap)]
#[unwrap(ref, ref_mut)]
#[try_unwrap(ref, ref_mut)]
pub enum Action {
  /// Indicates the token belongs to another syntactic element, hint to stop parsing.
  #[unwrap(ignore)]
  #[try_unwrap(ignore)]
  Stop,
  /// Indicates a token belongs to an element was found, hint to continue parsing.
  #[unwrap(ignore)]
  #[try_unwrap(ignore)]
  Continue,
}

#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "logos", feature = "std"))]
mod terminal_stop_tests;
