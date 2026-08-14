//! Common types for building language-specific ASTs.
//!
//! This module provides generic, reusable building blocks for creating Abstract
//! Syntax Trees (ASTs) across different programming languages. These types are
//! designed to be language-agnostic while maintaining type safety through generic
//! parameters.
//!
//! # Available Types
//!
//! ## Identifiers
//!
//! - `Ident`: Generic identifier with span tracking and language marker
//!
//! ## Generic Literal
//!
//! - `Lit`: Generic literal (any literal type)
//!
//! ## Numeric Literals
//!
//! - `LitDecimal`: Decimal integer (e.g., `42`, `1_000`)
//! - `LitHex`: Hexadecimal integer (e.g., `0xFF`, `0x1A2B`)
//! - `LitOctal`: Octal integer (e.g., `0o77`, `0o644`)
//! - `LitBinary`: Binary integer (e.g., `0b1010`)
//! - `LitFloat`: Floating-point (e.g., `3.14`, `1.0e-5`)
//! - `LitHexFloat`: Hexadecimal float (e.g., `0x1.8p3`)
//!
//! ## String Literals
//!
//! - `LitString`: Single-line string (e.g., `"hello"`)
//! - `LitMultilineString`: Multi-line string (e.g., `"""..."""`)
//! - `LitRawString`: Raw string (e.g., `r"C:\path"`)
//!
//! ## Character/Byte Literals
//!
//! - `LitChar`: Character literal (e.g., `'a'`)
//! - `LitByte`: Byte literal (e.g., `b'a'`)
//! - `LitByteString`: Byte string (e.g., `b"bytes"`)
//!
//! ## Boolean and Null
//!
//! - `LitBool`: Boolean literal (`true`/`false`)
//! - `LitNull`: Null/nil literal
//!
//! # Design Principles
//!
//! All types in this module follow these principles:
//!
//! 1. **Generic over string representation**: Support zero-copy (`&str`), owned
//!    (`String`), and interned strings
//! 2. **Span tracking**: Every node carries its source location for diagnostics
//! 3. **Language safety**: Generic `Lang` parameter prevents mixing ASTs from
//!    different languages
//! 4. **Error recovery**: Implement [`ErrorNode`](crate::error::ErrorNode) when
//!    appropriate for creating placeholders during parsing errors
//!
//! # Example: Building a Simple Expression AST
//!
//! ```rust,ignore
//! use tokora::types::Ident;
//! use tokora::utils::SimpleSpan;
//!
//! // Define your language marker
//! struct MyLang;
//!
//! // Define expression type using Ident
//! enum Expr<'a> {
//!     Variable(Ident<&'a str, MyLang>),
//!     Number(i64, SimpleSpan),
//!     Add(Box<Expr<'a>>, Box<Expr<'a>>, SimpleSpan),
//! }
//!
//! // Create an expression
//! let var = Ident::new(SimpleSpan::new(0, 1), "x");
//! let expr = Expr::Variable(var);
//! ```

use derive_more::{IsVariant, TryUnwrap, Unwrap};

use crate::{
  error::ErrorNode,
  span::{AsSpan, SimpleSpan},
  syntax::{Language, Syntax},
};

pub use ident::*;
pub use ident_list::*;
pub use keyword::*;
pub use lit::*;

mod ident;
mod ident_list;
mod keyword;
mod lit;

/// A type representing a recoverable parse node, which can be a valid node,
/// an error node with span, or a missing node with span.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, IsVariant, TryUnwrap, Unwrap)]
#[unwrap(ref, ref_mut)]
#[try_unwrap(ref, ref_mut)]
pub enum Recoverable<T, S = SimpleSpan> {
  /// A valid parse node.
  Node(T),
  /// An error node with associated span.
  Error(S),
  /// A missing node with associated span.
  Missing(S),
}

impl<T, S> AsSpan<S> for Recoverable<T, S>
where
  T: AsSpan<S>,
{
  #[inline(always)]
  fn as_span(&self) -> &S {
    match self {
      Self::Node(node) => node.as_span(),
      Self::Error(span) | Self::Missing(span) => span,
    }
  }
}

impl<T, S> Syntax for Recoverable<T, S>
where
  T: Syntax,
{
  type Lang = T::Lang;
  const KIND: <Self::Lang as Language>::SyntaxKind = T::KIND;

  type Component = T::Component;

  type COMPONENTS = T::COMPONENTS;

  type REQUIRED = T::REQUIRED;

  fn possible_components()
  -> &'static generic_arraydeque::GenericArrayDeque<Self::Component, Self::COMPONENTS> {
    T::possible_components()
  }

  fn required_components()
  -> &'static generic_arraydeque::GenericArrayDeque<Self::Component, Self::REQUIRED> {
    T::required_components()
  }
}

impl<T> ErrorNode for Recoverable<T> {
  #[inline(always)]
  fn error(span: SimpleSpan) -> Self {
    Self::Error(span)
  }

  #[inline(always)]
  fn missing(span: SimpleSpan) -> Self {
    Self::Missing(span)
  }
}

impl<T> From<T> for Recoverable<T> {
  #[inline(always)]
  fn from(node: T) -> Self {
    Self::Node(node)
  }
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
