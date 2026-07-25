pub use errors::{DefaultContainer, Errors};
pub use hex_escape::*;
pub use incomplete::*;
pub use incomplete_syntax::*;
pub use incomplete_token::*;
pub use invalid::*;
pub use invalid_hex_digits::*;
pub use malformed::*;
pub use missing::*;
pub use terminal::MaybeTerminal;
pub use unclosed::*;
pub use undelimited::*;
pub use unexpected_end::*;
pub use unexpected_identifier::*;
pub use unexpected_keyword::*;
pub use unexpected_lexeme::*;
pub use unexpected_prefix::*;
pub use unexpected_suffix::*;

pub use unicode_escape::*;
pub use unknown_lexeme::*;
pub use unopened::*;
pub use unterminated::*;

use generic_arraydeque::{ArrayLength, GenericArrayDeque};

use crate::span::SimpleSpan;

mod errors;

/// Token-level error types.
pub mod token;

/// Syntax-level error types.
pub mod syntax;

mod hex_escape;
mod incomplete;
mod incomplete_syntax;
mod incomplete_token;
mod invalid;
mod malformed;
mod missing;
mod terminal;
mod unclosed;
mod undelimited;
mod unexpected_end;
mod unexpected_identifier;
mod unexpected_keyword;
mod unexpected_lexeme;
mod unexpected_prefix;
mod unexpected_suffix;
mod unknown_lexeme;
mod unopened;
mod unterminated;

mod invalid_hex_digits;
mod unicode_escape;

/// Helper trait for producing placeholder AST/CST nodes during error recovery.
///
/// This trait enables parsers to create sentinel values when they encounter malformed or missing
/// syntax, allowing parsing to continue while marking problematic regions. Downstream passes can
/// detect these sentinel nodes and handle them appropriately (skip, report, attempt fix, etc.)
/// without disrupting the overall AST/CST structure.
///
/// # Two Flavors of Error Nodes
///
/// The trait provides two distinct methods for different recovery scenarios:
///
/// ## [`error`](Self::error) - Malformed Content
///
/// Use when **invalid syntax is present** that cannot be parsed correctly:
///
/// - **Parser found something**: Tokens exist but are malformed
/// - **Examples**: `let x = = 5;` (double equals), `fn 123foo()` (digit in identifier)
/// - **Semantic**: "Something is here, but it's wrong"
/// - **Typical placeholder**: `<error>`, `<malformed>`, `Error`
///
/// ## [`missing`](Self::missing) - Absent Required Content
///
/// Use when **required syntax is completely absent**:
///
/// - **Parser found nothing**: Expected tokens are missing entirely
/// - **Examples**: `let = 5;` (missing identifier), `fn ()` (missing name)
/// - **Semantic**: "Something should be here, but it's not"
/// - **Typical placeholder**: `<missing>`, `<absent>`, `Missing`
///
/// # Design Philosophy
///
/// This distinction helps with:
///
/// - **Better diagnostics**: Tools can show "malformed X" vs "missing X"
/// - **Refactoring assistance**: IDEs can offer different quick-fixes
/// - **Error recovery**: Different strategies for malformed vs missing syntax
/// - **Code generation**: Generate placeholder code (`todo!()`, `???`, etc.)
///
/// # Integration with Recovery Parsers
///
/// `ErrorNode` works seamlessly with Chumsky's recovery combinators:
///
/// ```rust,ignore
/// identifier_parser
///     .recover_with(via_parser(
///         // Malformed identifier (e.g., "123abc")
///         just(Token::InvalidIdent)
///             .to(Identifier::error(exa.span()))
///     ))
///     .or_else(|_| {
///         // Missing identifier entirely
///         Ok(Identifier::missing(exa.span()))
///     })
/// ```
///
/// # Examples
///
/// ## Basic Implementation
///
/// ```rust
/// use tokora::{error::ErrorNode, SimpleSpan};
///
/// #[derive(Debug, Clone, PartialEq)]
/// struct Identifier(String);
///
/// impl ErrorNode for Identifier {
///     fn error(_span: SimpleSpan) -> Self {
///         // Token was present but malformed (e.g., "123abc")
///         Identifier("<error>".to_string())
///     }
///
///     fn missing(_span: SimpleSpan) -> Self {
///         // Required identifier was completely absent
///         Identifier("<missing>".to_string())
///     }
/// }
///
/// // Parser encounters "let 123 = 5;"
/// let malformed = Identifier::error(SimpleSpan::new(4, 7)); // "123" is malformed
/// assert_eq!(malformed.0, "<error>");
///
/// // Parser encounters "let = 5;"
/// let absent = Identifier::missing(SimpleSpan::new(4, 4)); // Nothing where identifier expected
/// assert_eq!(absent.0, "<missing>");
/// ```
///
/// ## Enum-Based Error Nodes
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// enum Expression {
///     Number(i64),
///     Identifier(String),
///     Binary { op: Op, left: Box<Expr>, right: Box<Expr> },
///     Error,   // Malformed expression
///     Missing, // Missing required expression
/// }
///
/// impl ErrorNode for Expression {
///     fn error(_span: Span) -> Self {
///         Expression::Error
///     }
///
///     fn missing(_span: Span) -> Self {
///         Expression::Missing
///     }
/// }
/// ```
///
/// ## Span-Aware Error Nodes
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct TypeAnnotation {
///     name: String,
///     span: Span,
/// }
///
/// impl ErrorNode for TypeAnnotation {
///     fn error(span: Span) -> Self {
///         // Keep span for precise error reporting
///         TypeAnnotation {
///             name: "<error>".to_string(),
///             span,
///         }
///     }
///
///     fn missing(span: Span) -> Self {
///         TypeAnnotation {
///             name: "<missing>".to_string(),
///             span,
///         }
///     }
/// }
/// ```
///
/// ## Distinguishing Errors in Diagnostics
///
/// ```rust,ignore
/// fn report_errors(ast: &Program) {
///     for node in ast.nodes() {
///         match node {
///             Node::Identifier(id) if id.0 == "<error>" => {
///                 eprintln!("error: malformed identifier at {:?}", node.span());
///                 eprintln!("help: identifiers cannot start with digits");
///             }
///             Node::Identifier(id) if id.0 == "<missing>" => {
///                 eprintln!("error: expected identifier at {:?}", node.span());
///                 eprintln!("help: insert a name here");
///             }
///             _ => {}
///         }
///     }
/// }
/// ```
///
/// # Best Practices
///
/// ## Use Consistent Sentinel Values
///
/// Choose recognizable placeholder values that:
/// - Cannot occur in valid source code
/// - Are easy to detect in downstream passes
/// - Clearly indicate error vs missing
///
/// ## Preserve Span Information
///
/// Always store the span parameter if your AST nodes track positions:
///
/// ```rust,ignore
/// impl ErrorNode for MyNode {
///     fn error(span: Span) -> Self {
///         MyNode { value: "<error>".into(), span } // ✅ Good
///     }
///
///     fn missing(span: Span) -> Self {
///         MyNode { value: "<missing>".into(), span: Span::default() } // ❌ Lost position!
///     }
/// }
/// ```
///
/// ## Document Recovery Behavior
///
/// Explain how your parser uses error nodes:
///
/// ```rust,ignore
/// /// Parses a function declaration.
/// ///
/// /// # Recovery
/// ///
/// /// - Malformed name (e.g., "123func"): Creates Identifier::error()
/// /// - Missing name: Creates Identifier::missing()
/// /// - Missing parameters: Creates Parameters::missing()
/// fn parse_function() -> Function { ... }
/// ```
///
/// # See Also
///
/// - [`Recover`](crate::parser::Recover): Recovery combinator with backtracking
/// - [`InplaceRecover`](crate::parser::InplaceRecover): Recovery combinator without backtracking
pub trait ErrorNode<S = SimpleSpan> {
  /// Creates a placeholder node for **malformed content**.
  ///
  /// Use this when the parser encounters **invalid syntax that is present but wrong**.
  /// The span typically covers the malformed tokens that were found.
  ///
  /// # When to Use
  ///
  /// - Parser found tokens but they don't match expected syntax
  /// - Content is present but structurally incorrect
  /// - Something exists where it shouldn't, or in the wrong form
  ///
  /// # Examples
  ///
  /// ```text
  /// let 123 = 5;        // "123" is malformed identifier
  /// fn if() { }         // "if" is keyword used as name (malformed)
  /// x + * y             // "*" without left operand (malformed binary op)
  /// ```
  ///
  /// # Implementation
  ///
  /// ```rust
  /// use tokora::{error::ErrorNode, SimpleSpan};
  ///
  /// struct Identifier(String);
  ///
  /// impl ErrorNode for Identifier {
  ///     fn error(_span: SimpleSpan) -> Self {
  ///         Identifier("<error>".to_string())
  ///     }
  ///
  ///     fn missing(_span: SimpleSpan) -> Self {
  ///         Identifier("<missing>".to_string())
  ///     }
  /// }
  ///
  /// // Parser found "123abc" as identifier
  /// let node = Identifier::error(SimpleSpan::new(0, 6));
  /// ```
  fn error(span: S) -> Self;

  /// Creates a placeholder node for **missing required content**.
  ///
  /// Use this when the parser expects syntax but **finds nothing at all**.
  /// The span typically points to where the content should have been.
  ///
  /// # When to Use
  ///
  /// - Parser expected tokens but found none
  /// - Required syntax element is completely absent
  /// - Gap exists where content should be
  ///
  /// # Examples
  ///
  /// ```text
  /// let = 5;            // Missing identifier entirely
  /// fn () { }           // Missing function name
  /// x +                 // Missing right operand
  /// ```
  ///
  /// # Implementation
  ///
  /// ```rust
  /// use tokora::{error::ErrorNode, SimpleSpan};
  ///
  /// struct FunctionName(String);
  ///
  /// impl ErrorNode for FunctionName {
  ///     fn error(_span: SimpleSpan) -> Self {
  ///         FunctionName("<error>".to_string())
  ///     }
  ///
  ///     fn missing(_span: SimpleSpan) -> Self {
  ///         FunctionName("<missing>".to_string())
  ///     }
  /// }
  ///
  /// // Parser expected function name but found "("
  /// let node = FunctionName::missing(SimpleSpan::new(3, 3));
  /// ```
  fn missing(span: S) -> Self;
}

impl ErrorNode for &str {
  #[inline(always)]
  fn error(_span: SimpleSpan) -> Self {
    "<error>"
  }

  #[inline(always)]
  fn missing(_span: SimpleSpan) -> Self {
    "<missing>"
  }
}

impl ErrorNode for &[u8] {
  #[inline(always)]
  fn error(_span: SimpleSpan) -> Self {
    b"<error>"
  }

  #[inline(always)]
  fn missing(_span: SimpleSpan) -> Self {
    b"<missing>"
  }
}

#[cfg(feature = "bytes_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bytes_1")))]
impl ErrorNode for bytes_1::Bytes {
  #[inline(always)]
  fn error(_span: SimpleSpan) -> Self {
    bytes_1::Bytes::from_static(b"<error>")
  }

  #[inline(always)]
  fn missing(_span: SimpleSpan) -> Self {
    bytes_1::Bytes::from_static(b"<missing>")
  }
}

#[cfg(feature = "hipstr_0_8")]
#[cfg_attr(docsrs, doc(cfg(feature = "hipstr_0_8")))]
const _: () = {
  use hipstr_0_8::{HipByt, HipStr};

  impl ErrorNode for HipStr<'_> {
    #[inline(always)]
    fn error(_span: SimpleSpan) -> Self {
      HipStr::borrowed("<error>")
    }

    #[inline(always)]
    fn missing(_span: SimpleSpan) -> Self {
      HipStr::borrowed("<missing>")
    }
  }

  impl ErrorNode for HipByt<'_> {
    #[inline(always)]
    fn error(_span: SimpleSpan) -> Self {
      HipByt::borrowed(b"<error>")
    }

    #[inline(always)]
    fn missing(_span: SimpleSpan) -> Self {
      HipByt::borrowed(b"<missing>")
    }
  }
};

/// A container of error types
pub trait ErrorContainer<E> {
  /// The iterator type for the container.
  type IntoIter: Iterator<Item = E>;
  /// The iterator type for references to the container.
  type Iter<'a>: Iterator<Item = &'a E>
  where
    Self: 'a,
    E: 'a;

  /// Create a new, empty container.
  fn new() -> Self;

  /// Create a new container with a specified capacity.
  #[inline(always)]
  fn with_capacity(_: usize) -> Self
  where
    Self: Sized,
  {
    Self::new()
  }

  /// Push an error into the collection.
  fn push(&mut self, error: E);

  /// Attempts to push an error, returning it back if the container is full.
  #[inline(always)]
  fn try_push(&mut self, error: E) -> Result<(), E>
  where
    Self: Sized,
  {
    self.push(error);
    Ok(())
  }

  /// Pop an error from the first of the collection.
  fn pop(&mut self) -> Option<E>;

  /// Returns `true` if the collection is empty.
  #[inline(always)]
  fn is_empty(&self) -> bool {
    self.len() == 0
  }

  /// Returns the number of errors in the collection.
  fn len(&self) -> usize;

  /// Returns an iterator over the errors in the collection.
  fn iter(&self) -> Self::Iter<'_>;

  /// Consumes the container and returns an iterator over the errors.
  fn into_iter(self) -> Self::IntoIter;

  /// Returns the remaining capacity if the container has a fixed upper bound.
  #[inline(always)]
  fn remaining_capacity(&self) -> Option<usize> {
    None
  }
}

impl<E> ErrorContainer<E> for Option<E> {
  type IntoIter = core::option::IntoIter<E>;
  type Iter<'a>
    = core::option::Iter<'a, E>
  where
    E: 'a;

  #[inline(always)]
  fn new() -> Self {
    None
  }

  #[inline(always)]
  fn push(&mut self, error: E) {
    self.get_or_insert(error);
  }

  #[inline(always)]
  fn try_push(&mut self, error: E) -> Result<(), E> {
    if self.is_some() {
      Err(error)
    } else {
      self.push(error);
      Ok(())
    }
  }

  #[inline(always)]
  fn pop(&mut self) -> Option<E> {
    self.take()
  }

  #[inline(always)]
  fn len(&self) -> usize {
    if self.is_some() { 1 } else { 0 }
  }

  #[inline(always)]
  fn iter(&self) -> Self::Iter<'_> {
    Self::iter(self)
  }

  #[inline(always)]
  fn into_iter(self) -> Self::IntoIter {
    <Self as IntoIterator>::into_iter(self)
  }

  #[inline(always)]
  fn remaining_capacity(&self) -> Option<usize> {
    Some(if self.is_some() { 0 } else { 1 })
  }
}

impl<E, N: ArrayLength> ErrorContainer<E> for GenericArrayDeque<E, N> {
  type IntoIter = generic_arraydeque::IntoIter<E, N>;

  type Iter<'a>
    = generic_arraydeque::Iter<'a, E>
  where
    Self: 'a,
    E: 'a;

  #[inline(always)]
  fn new() -> Self {
    Self::new()
  }

  #[inline(always)]
  fn push(&mut self, error: E) {
    self.push_back(error);
  }

  #[inline(always)]
  fn try_push(&mut self, error: E) -> Result<(), E> {
    match self.push_back(error) {
      None => Ok(()),
      Some(e) => Err(e),
    }
  }

  #[inline(always)]
  fn pop(&mut self) -> Option<E> {
    self.pop_front()
  }

  #[inline(always)]
  fn len(&self) -> usize {
    self.len()
  }

  #[inline(always)]
  fn iter(&self) -> Self::Iter<'_> {
    Self::iter(self)
  }

  #[inline(always)]
  fn into_iter(self) -> Self::IntoIter {
    IntoIterator::into_iter(self)
  }

  #[inline(always)]
  fn remaining_capacity(&self) -> Option<usize> {
    Some(self.remaining_capacity())
  }
}

#[cfg(any(feature = "std", feature = "alloc"))]
const _: () = {
  use std::{
    collections::{VecDeque, vec_deque},
    vec::{self, Vec},
  };

  impl<E> ErrorContainer<E> for Vec<E> {
    type IntoIter = vec::IntoIter<E>;
    type Iter<'a>
      = core::slice::Iter<'a, E>
    where
      E: 'a;

    #[inline(always)]
    fn new() -> Self {
      Self::new()
    }

    #[inline(always)]
    fn with_capacity(capacity: usize) -> Self {
      Self::with_capacity(capacity)
    }

    #[inline(always)]
    fn push(&mut self, error: E) {
      self.push(error);
    }

    #[inline(always)]
    fn pop(&mut self) -> Option<E> {
      if self.is_empty() {
        None
      } else {
        Some(self.remove(0))
      }
    }

    #[inline(always)]
    fn len(&self) -> usize {
      self.len()
    }

    #[inline(always)]
    fn iter(&self) -> Self::Iter<'_> {
      self.as_slice().iter()
    }

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
      <Self as IntoIterator>::into_iter(self)
    }
  }

  impl<E> ErrorContainer<E> for VecDeque<E> {
    type IntoIter = vec_deque::IntoIter<E>;
    type Iter<'a>
      = vec_deque::Iter<'a, E>
    where
      E: 'a,
      Self: 'a;

    #[inline(always)]
    fn new() -> Self {
      Self::new()
    }

    #[inline(always)]
    fn with_capacity(capacity: usize) -> Self {
      Self::with_capacity(capacity)
    }

    #[inline(always)]
    fn push(&mut self, error: E) {
      self.push_back(error);
    }

    #[inline(always)]
    fn pop(&mut self) -> Option<E> {
      self.pop_front()
    }

    #[inline(always)]
    fn len(&self) -> usize {
      self.len()
    }

    #[inline(always)]
    fn iter(&self) -> Self::Iter<'_> {
      self.iter()
    }

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
      <Self as IntoIterator>::into_iter(self)
    }
  }
};

#[cfg(test)]
#[allow(warnings)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
