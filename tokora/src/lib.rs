#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, allow(unused_attributes))]
#![allow(clippy::double_parens, clippy::type_complexity)]
#![deny(missing_docs, warnings)]
// With `unstable-raw` off, `InputRef::{save, restore, commit}` are `pub(crate)`, so the many
// public items documenting the raw checkpoint contract (the `Checkpoint` type, the transaction
// guards, `ParseState`, `attempt`) link to crate-private methods. Those links are intentionally
// inert in that build and fully live under the feature (and on docs.rs, which builds all
// features); relax the lint only when the feature is off so the on-feature docs stay strict.
#![cfg_attr(not(feature = "unstable-raw"), allow(rustdoc::private_intra_doc_links))]

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc as std;

#[cfg(feature = "std")]
extern crate std;

// Declared before the combinator modules so the internal `trace_event!` hook macro is in
// scope for them. With the `trace` feature off the macro expands to nothing, so every hook
// site compiles away entirely.
#[macro_use]
mod trace;

#[cfg(feature = "logos_0_16")]
#[cfg_attr(docsrs, doc(cfg(feature = "logos_0_16")))]
pub use logos_0_16 as logos;

pub use cache::{Cache, DefaultCache};
pub use check::Check;
pub use dialect::{Dialect, DialectErrorOf, DialectInput, DialectSlice, LangOf, LexerOf};
pub use emitter::{ComposableEmitter, CstEmitter, Emitter};
pub use input::{
  Balance, Commit, Complete, Completeness, DelimClass, DropPolicy, Hole, InputRef, Partial,
  Rollback, SurfaceIncomplete, Transaction, parse_partial,
};
#[cfg(any(feature = "std", feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
pub use input::{SavepointId, SessionPointId, StackedTransaction};
pub use lexer::{Lexed, Lexer, SliceOf};
pub use located::*;
pub use parse_choice::*;
pub use parse_context::{
  ComposableParseContext, ErrorOf, FatalContext, ParseContext, ParserContext,
};
pub use parse_input::*;
pub use parse_state::ParseState;
pub use parser::{Labelled, Parse, Parser, labelled, parse, parse_with, parse_with_state};
pub use require::Require;
pub use slice::Slice;
pub use source::Source;
pub use span::{SimpleSpan, Span};
pub use state::State;
pub use token::Token;
pub use try_parse_input::TryParseInput;

#[cfg(feature = "trace")]
#[cfg_attr(docsrs, doc(cfg(feature = "trace")))]
pub use trace::Traced;
/// Parser tracing DX: wrap any parser in [`traced`] to print an indented event tree of its
/// run — enter, exit-ok with the consumed span, exit-err — interleaved with the crate's own
/// instrumented combinators (`try_expect`, `peek`, the `sync` family, the transaction guards,
/// `attempt`/`try_attempt`, and the separated/repeated drivers) as they fire. Gated on the
/// `trace` feature; with it off, [`traced`] is the identity and every hook compiles away.
pub use trace::traced;

/// Concrete Syntax Tree (CST) representations and utilities.
///
/// The rowan-free front half — the [`cst::event`] vocabulary, the era-branded marks, and the
/// [`emitter::CstEmitter`](crate::emitter::CstEmitter) capability they ride — is available in
/// every build, so one parser assembly can bound its context on the event channel and run
/// tree-less at zero cost. The `rowan` feature adds the back half: the recording sink and the
/// typed lossless-tree layer (preserving all source information including whitespace and
/// comments) for building tools like formatters, refactoring engines, and language servers.
pub mod cst;

/// Lexical analysis and token extraction.
///
/// Contains the [`Lexer`] trait and [`Lexed`] type for converting source text into tokens.
/// Tokens flow on demand to parsers and are cached only when explicit lookahead or backtracking
/// needs them.
pub mod lexer;

/// Parser combinators with on-demand lexing, explicit lookahead, and deterministic parsing.
///
/// A unique parser combinator framework combining:
/// - **Parse-while-lexing architecture**: Tokens are requested from the lexer as parsing needs them
/// - **Explicit lookahead and backtracking**: Caches and transaction guards make speculation visible
/// - **Flexible error handling**: Same parser adapts for fail-fast or greedy diagnostics
///
/// See the module documentation for architecture details and quick start guide.
pub mod parser;

/// Common AST building blocks for programming languages.
///
/// Provides generic, reusable types for building Abstract Syntax Trees across different
/// programming languages. Includes identifiers, literals (numeric, string, character),
/// and other common AST nodes. All types support span tracking and are generic over
/// string representation (zero-copy `&str`, owned `String`, or interned strings).
pub mod types;

/// Syntax definition and incomplete syntax error tracking.
///
/// Provides the [`Syntax`](syntax::Syntax) trait for representing syntax elements with a
/// known number of components, and error types for tracking missing components during
/// parsing. Enables collecting all missing parts rather than failing on the first error,
/// providing better diagnostics.
pub mod syntax;

/// Utility types and helpers for lexing and parsing.
///
/// Contains common utilities including:
/// - Generic array deque and type-level numbers (re-exported from `generic-arraydeque`)
/// - Delimited and escaped sequence helpers
/// - Display traits for human-readable, SDL, and syntax tree output
/// - Positioned character iterators
/// - Message and knowledge types for error reporting
pub mod utils;

/// Container trait for accumulating parsed results.
///
/// Defines the [`Container`](container::Container) trait for types that can accumulate
/// parsing results. Implemented for standard collections like `Vec`, arrays, and
/// `GenericArrayDeque`, enabling parsers to collect multiple elements into containers.
pub mod container;

/// Atomically composable error handling and reporting.
///
/// Provides the [`Emitter`] trait and related traits for flexible error handling during
/// parsing. The atomic design allows implementing only needed traits for specific use
/// cases. Includes pre-built emitters:
/// - [`Fatal`](emitter::Fatal): Fail-fast on first error (for runtime/REPL)
/// - [`Verbose`](emitter::Verbose): Collect all errors (for compiler diagnostics)
/// - [`Silent`](emitter::Silent): Suppress errors (for speculative parsing)
pub mod emitter;

/// Comprehensive error types for lexer and parser diagnostics.
///
/// Contains detailed error types organized by category:
/// - **Token errors**: Unexpected tokens, missing/extra separators
/// - **Lexer errors**: Unknown lexemes, malformed literals, invalid escape sequences
/// - **Syntax errors**: Incomplete syntax, too few/many elements, container overflow
/// - **Delimiter errors**: Unclosed/unopened/undelimited constructs
///
/// All errors carry span information for precise diagnostic reporting.
pub mod error;

/// Macro for defining punctuator types.
///
/// Provides the [`punctuator!`] macro for generating zero-sized punctuator types with
/// span tracking. Punctuators are generic over span and source types, enabling both
/// phantom (zero-size) and concrete instances for use in ASTs.
pub mod punct;

/// Delimiter types and utilities.
///
/// Defines common delimiter types (brackets, braces, parentheses) and utilities
/// for working with delimited constructs in parsing.
pub mod delimiter;

/// The dialect anchor: one type per language, naming its lexer and its brand.
///
/// A grammar's productions all need the same block of lexer equalities — source, slice, token,
/// span, offset, token capabilities — and Rust cannot name that block once. The [`Dialect`]
/// trait is the anchor it hangs off: a consumer writes one impl per language, pins the
/// projections in a one-line subtrait, and each production then carries two `where`-clause
/// lines. The [`LexerOf`] / [`LangOf`] / [`DialectSlice`] / [`DialectInput`] /
/// [`DialectErrorOf`] aliases spell the projections a signature actually mentions.
pub mod dialect;

/// Source text abstraction for lexers.
///
/// Defines the [`Source`] trait for accessing source text during lexing.
/// Supports both string (`&str`) and byte (`&[u8]`) sources with proper boundary
/// checking. Handles UTF-8 character boundaries for string sources and byte boundaries
/// for binary sources.
pub mod source;

/// Source location tracking and span types.
///
/// Defines the [`span::Span`] trait for representing source code ranges with
/// start and end offsets. Provides operations for creating, manipulating, and querying
/// spans. Implemented for `Range<usize>` and custom span types.
pub mod span;

/// Slice abstractions for different string types.
///
/// Defines the [`Slice`] trait for working with string slices in a
/// generic way. Supports multiple string types through feature flags:
/// - `bytes`: `&[u8]` (byte slices)
/// - `bstr`: `bstr::BStr` (byte strings)
/// - `hipstr`: `hipstr::HipStr` (inline/heap strings)
pub mod slice;

/// State management for lexers.
///
/// Provides state tracking types for lexers:
/// - [`State`]: Base trait for lexer state
/// - [`recursion_tracker`](state::recursion_tracker): Prevent infinite recursion
/// - [`token_tracker`](state::token_tracker): Track token occurrences
/// - [`tracker`](state::tracker): Combined recursion and token tracking
pub mod state;

/// Token caching for lookahead and backtracking.
///
/// Defines the [`Cache`] trait for buffering tokens to enable lookahead
/// and backtracking operations. Provides implementations for:
/// - Fixed-size arrays: Bounded lookahead with known maximum capacity
/// - Dynamic buffers: Unlimited lookahead (when `alloc` feature is enabled)
/// - Black hole cache: No caching for streaming-only scenarios
pub mod cache;

/// Token trait and related types.
///
/// Defines the [`Token`] trait that bridges lexical analysis (Logos) and
/// structured token representation for parsing. Separates raw lexer output from the
/// token type used in parsing, allowing custom data and behavior beyond what Logos
/// provides.
pub mod token;

/// Input stream abstraction for parsers.
///
/// Provides the [`InputRef`] type that bridges lexers and parsers,
/// implementing on-demand token flow. Maintains cursor position, lookahead caches, and
/// checkpoint/rewind capabilities for explicit backtracking.
pub mod input;

/// Conformance test kit for custom [`Lexer`] implementations.
///
/// Provides [`Harness`](conformance::Harness), a builder that drives a lexer against
/// the [`Lexer`] contract — replay identity, state-resume faithfulness, monotone
/// progress, sticky exhaustion, span/slice coherence, optional gap-free tiling — and,
/// through the input machinery, a set of deterministic save/peek/drain/restore
/// schedules. Requires the `conformance` feature (which implies `std`).
#[cfg(feature = "conformance")]
#[cfg_attr(docsrs, doc(cfg(feature = "conformance")))]
pub mod conformance;

/// Public fuzz harness for the input/backtracking machinery.
///
/// Provides an operation-script fuzzer — a deterministic PRNG drives well-formed scripts of
/// the crate's public input operations (consume, peek, the `sync` family, `attempt`,
/// transaction guards, stacked savepoints, session points, partial-mode chunking) against a
/// scriptable synthetic lexer, checking the documented laws (no-trace failure paths, LIFO
/// rollback discipline, committed-stream faithfulness, chunked equivalence, termination, no
/// panic) after every operation. The operation alphabet is enumerated in one place
/// ([`fuzz::Op`](crate::fuzz::Op)) with a compile-time exhaustiveness prod and a corpus coverage
/// test so it cannot silently lag the real surface. Runs on stable Rust as ordinary tests;
/// requires the `fuzz` feature (which implies `std`). See [`fuzz`](crate::fuzz) for the seed
/// workflow.
#[cfg(feature = "fuzz")]
#[cfg_attr(docsrs, doc(cfg(feature = "fuzz")))]
pub mod fuzz;

/// A guided tour of tokora: from Calc fundamentals to maintained parser programs and lossless CSTs.
///
/// The fundamentals build **Calc** through tokens and the lexer contract, first parsers and
/// typed errors, combinator composition, kind dispatch, Pratt expressions, backtracking,
/// diagnostics, recovery, partial input, and testing. The applied-parser section then explains
/// the maintained calculator, S-expression, JSON, and C-expression examples. An optional Rowan
/// chapter covers lossless CSTs. Every non-ignored Rust fence is a doctest, so the guide cannot
/// drift quietly from the API. Start at [`guide::ch01_tokens`](guide::ch01_tokens).
///
/// Documentation-only: the module defines no items. It requires the `std` and `logos`
/// features (the same set the repository's `examples/` build with).
#[cfg(all(feature = "std", feature = "logos_0_16"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "std", feature = "logos_0_16"))))]
pub mod guide;

/// Convenience re-exports for common usage.
pub mod prelude;

/// Tentative parsing trait
pub mod try_parse_input;

mod check;
mod keyword;
mod located;
mod parse_choice;
mod parse_context;
mod parse_input;
mod parse_state;
mod require;

#[doc(hidden)]
pub mod __private {
  pub use super::{check::Check, error, lexer::*, require::Require, span, syntax, token, utils};

  #[cfg(feature = "logos_0_16")]
  pub use ::logos_0_16 as logos;
  pub use paste;

  #[cfg(any(feature = "std", feature = "alloc"))]
  pub use std::{boxed::Box, string::String, vec::Vec};
}
