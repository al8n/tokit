//! Literal token types for language syntax trees.
//!
//! This module provides generic literal types that represent various kinds of
//! literal values found in programming languages: numbers, strings, booleans, etc.
//! Each literal type carries its source representation along with span information.
//!
//! # Design Philosophy
//!
//! All literal types follow the same pattern as [`Ident`](super::Ident):
//!
//! - **Generic string type `S`**: Support `&str`, `String`, or interned strings
//! - **Language marker `Lang`**: Type-safe language distinction
//! - **Span tracking**: All literals carry source location for diagnostics
//! - **Error recovery**: Implement [`ErrorNode`] for placeholder creation
//!
//! # Available Literal Types
//!
//! ## Generic Literal
//!
//! - [`Lit`]: Generic literal (any literal type)
//!
//! ## Numeric Literals
//!
//! - [`LitDecimal`]: Base-10 integer (e.g., `42`, `1_000`)
//! - [`LitHex`]: Hexadecimal integer (e.g., `0xFF`, `0x1A2B`)
//! - [`LitOctal`]: Octal integer (e.g., `0o77`, `0o644`)
//! - [`LitBinary`]: Binary integer (e.g., `0b1010`, `0b1111_0000`)
//! - [`LitFloat`](crate::types::lit::LitFloat): Floating-point (e.g., `3.14`, `1.0e-5`)
//! - [`LitHexFloat`]: Hexadecimal float (e.g., `0x1.8p3`)
//!
//! ## String Literals
//!
//! - [`LitString`]: Single-line string (e.g., `"hello"`)
//! - [`LitMultilineString`]: Multi-line string (e.g., `"""..."""`)
//! - [`LitRawString`]: Raw string without escape processing (e.g., `r"C:\path"`)
//!
//! ## Character/Byte Literals
//!
//! - [`LitChar`]: Character literal (e.g., `'a'`, `'\n'`)
//! - [`LitByte`]: Byte literal (e.g., `b'a'`, `b'\x7F'`)
//! - [`LitByteString`]: Byte string (e.g., `b"bytes"`)
//!
//! ## Boolean and Null
//!
//! - [`LitBool`]: Boolean literal (`true`/`false`)
//! - [`LitNull`]: Null/nil literal
//!
//! # Common Usage Patterns
//!
//! ## Zero-Copy Parsing
//!
//! ```rust,ignore
//! use tokora::types::{Lit, LitDecimal, LitString};
//! use tokora::SimpleSpan;
//!
//! // Parse literals without allocating
//! type YulLit<'a> = Lit<&'a str, SimpleSpan, YulLang>;
//! type YulDecimal<'a> = LitDecimal<&'a str, SimpleSpan, YulLang>;
//! type YulString<'a> = LitString<&'a str, SimpleSpan, YulLang>;
//!
//! let generic = YulLit::new(SimpleSpan::new(0, 2), "42");
//! let num = YulDecimal::new(SimpleSpan::new(0, 2), "42");
//! let str = YulString::new(SimpleSpan::new(5, 12), "\"hello\"");
//! ```
//!
//! ## Owned Literals
//!
//! ```rust,ignore
//! // Store literals in AST nodes
//! type OwnedDecimal = LitDecimal<String, SimpleSpan, MyLang>;
//!
//! let lit = OwnedDecimal::new(span, source.to_string());
//! ```
//!
//! # Error Recovery
//!
//! All literal types implement [`ErrorNode`] when `S: ErrorNode`:
//!
//! ```rust,ignore
//! use tokora::types::LitDecimal;
//! use tokora::error::ErrorNode;
//!
//! // Create placeholder for malformed literal
//! let bad_lit = LitDecimal::<String, SimpleSpan, YulLang>::error(span);
//!
//! // Create placeholder for missing literal
//! let missing_lit = LitDecimal::<String, SimpleSpan, YulLang>::missing(span);
//! ```

use core::marker::PhantomData;

use crate::{error::ErrorNode, span::AsSpan, utils::IntoComponents};

/// A macro to generate literal type structures.
///
/// This reduces boilerplate by generating identical structure and implementations
/// for all literal types.
macro_rules! define_literal {
  (
    $(#[$meta:meta])*
    $name:ident $(= $default:ty)?,
    $doc:expr,
    $example_str:expr,
    $example_desc:expr
  ) => {
    paste::paste! {
      $(#[$meta])*
      #[doc = $doc]
      ///
      /// # Type Parameters
      ///
      /// - `D`: The literal data type (`&str`, `String`, a parsed value, etc.)
      /// - `Span`: The span type tracking the literal's source location
      /// - `Lang`: Language marker type for type safety
      ///
      /// # Examples
      ///
      /// ## Creating Literals
      ///
      /// ```rust
      #[doc = "use tokora::types::" $name ";"]
      /// use tokora::SimpleSpan;
      /// # struct MyLang;
      ///
      #[doc = "let lit = " $name "::<&str, SimpleSpan, MyLang>::new("]
      #[doc = "    SimpleSpan::new(0, 4),"]
      #[doc = "    " $example_str ]
      /// );
      ///
      #[doc = "assert_eq!(lit.data_ref(), &" $example_str ");"]
      /// ```
      ///
      /// ## With Error Recovery
      ///
      /// ```rust,ignore
      #[doc = "use tokora::types::" $name ";"]
      /// use tokora::error::ErrorNode;
      ///
      #[doc = "// " $example_desc]
      #[doc = "let bad_lit = " $name "::<String, SimpleSpan, YulLang>::error(span);"]
      /// ```
      #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
      pub struct $name<
        D: ?::core::marker::Sized $( = $default)?,
        Span = $crate::__private::span::SimpleSpan,
        Lang: ?::core::marker::Sized = (),
      > {
        _lang: PhantomData<Lang>,
        span: Span,
        data: D,
      }
    }

    impl<D: ?::core::marker::Sized, Span, Lang: ?::core::marker::Sized> AsSpan<Span>
      for $name<D, Span, Lang>
    {
      #[inline(always)]
      fn as_span(&self) -> &Span {
        self.span_ref()
      }
    }

    impl<D, Span, Lang: ?::core::marker::Sized> IntoComponents for $name<D, Span, Lang> {
      type Components = (Span, D);

      #[inline(always)]
      fn into_components(self) -> Self::Components {
        (self.span, self.data)
      }
    }

    impl<D: ?::core::marker::Sized, Span, Lang: ?::core::marker::Sized> $name<D, Span, Lang> {
      /// Returns the span (source location) of this literal.
      #[inline(always)]
      pub const fn span(&self) -> Span where Span: ::core::marker::Copy {
        self.span
      }

      /// Returns an immutable reference to the span.
      #[inline(always)]
      pub const fn span_ref(&self) -> &Span {
        &self.span
      }

      /// Returns a mutable reference to the span.
      #[inline(always)]
      pub const fn span_mut(&mut self) -> &mut Span {
        &mut self.span
      }

      /// Bumps the span to of this literal by the specified offset.
      #[inline(always)]
      pub fn bump(&mut self, by: &Span::Offset) -> &mut Self
      where
        Span: $crate::__private::span::Span,
      {
        self.span.bump(by);
        self
      }

      /// Returns a mutable reference to the source string.
      #[inline(always)]
      pub const fn data_mut(&mut self) -> &mut D {
        &mut self.data
      }

      /// Returns an immutable reference to the source string.
      ///
      /// This is the most common way to access the literal's text.
      #[inline(always)]
      pub const fn data_ref(&self) -> &D {
        &self.data
      }
    }

    impl<D, Span, Lang: ?::core::marker::Sized> $name<D, Span, Lang> {
      /// Creates a new literal with the given span and source string.
      ///
      /// # Parameters
      ///
      /// - `span`: The source location of this literal
      /// - `data`: The literal's data
      #[inline(always)]
      pub const fn new(span: Span, data: D) -> Self {
        Self {
          span,
          data,
          _lang: PhantomData,
        }
      }

      /// Returns a copy of the source string by value.
      ///
      /// Only available when `D` implements [`Copy`].
      #[inline(always)]
      pub const fn data(&self) -> D
      where
        D: Copy,
      {
        self.data
      }
    }

    impl<D, Span, Lang: ?::core::marker::Sized> ErrorNode<Span> for $name<D, Span, Lang>
    where
      D: ErrorNode<Span>,
      Span: Clone,
    {
      /// Creates a placeholder literal for **malformed content**.
      #[inline(always)]
      fn error(span: Span) -> Self {
        Self::new(span.clone(), D::error(span))
      }

      /// Creates a placeholder literal for **missing required content**.
      #[inline(always)]
      fn missing(span: Span) -> Self {
        Self::new(span.clone(), D::missing(span))
      }
    }
  };
}

// Generic literal
define_literal!(
  /// A generic literal.
  ///
  /// Represents any kind of literal value without distinguishing between specific
  /// types (numeric, string, boolean, etc.). Useful when the exact literal type
  /// doesn't matter for your use case.
  Lit,
  "A generic literal (any literal type).",
  "\"value\"",
  "Malformed literal"
);

// Numeric literals
define_literal!(
  /// A decimal (base-10) integer literal.
  ///
  /// Represents numeric literals in standard decimal notation, such as `42`, `1000`,
  /// or `123_456`. The source string may include underscores for readability but
  /// represents a single integer value.
  LitDecimal,
  "A decimal integer literal (e.g., `42`, `1_000`).",
  "\"42\"",
  "Malformed decimal literal like \"12abc\""
);

define_literal!(
  /// A hexadecimal (base-16) integer literal.
  ///
  /// Represents integer literals in hexadecimal notation, typically prefixed with
  /// `0x` or `0X`, such as `0xFF`, `0x1A2B`, or `0xDEAD_BEEF`.
  LitHex,
  "A hexadecimal integer literal (e.g., `0xFF`, `0x1A2B`).",
  "\"0xFF\"",
  "Malformed hex literal like \"0xGG\""
);

define_literal!(
  /// An octal (base-8) integer literal.
  ///
  /// Represents integer literals in octal notation, typically prefixed with `0o`,
  /// such as `0o77`, `0o644`, or `0o755`.
  LitOctal,
  "An octal integer literal (e.g., `0o77`, `0o644`).",
  "\"0o77\"",
  "Malformed octal literal like \"0o89\""
);

define_literal!(
  /// A binary (base-2) integer literal.
  ///
  /// Represents integer literals in binary notation, typically prefixed with `0b`,
  /// such as `0b1010`, `0b11110000`, or `0b1111_0000`.
  LitBinary,
  "A binary integer literal (e.g., `0b1010`, `0b1111_0000`).",
  "\"0b1010\"",
  "Malformed binary literal like \"0b123\""
);

define_literal!(
  /// A floating-point literal.
  ///
  /// Represents floating-point literals in standard decimal notation with optional
  /// fractional and exponent parts, such as `3.14`, `1.0`, `2.5e-3`, or `6.022e23`.
  LitFloat,
  "A floating-point literal (e.g., `3.14`, `1.0e-5`).",
  "\"3.14\"",
  "Malformed float literal like \"3.14.15\""
);

define_literal!(
  /// A hexadecimal floating-point literal.
  ///
  /// Represents floating-point literals in hexadecimal notation with binary exponent,
  /// such as `0x1.8p3` (which equals 12.0 in decimal). Used in languages like C and Rust
  /// for precise floating-point representation.
  LitHexFloat,
  "A hexadecimal floating-point literal (e.g., `0x1.8p3`).",
  "\"0x1.8p3\"",
  "Malformed hex float like \"0x1.Gp3\""
);

// String literals
define_literal!(
  /// A single-line string literal.
  ///
  /// Represents string literals enclosed in quotes, typically on a single line,
  /// such as `"hello"`, `"world\n"`, or `"escaped \"quotes\""`. May contain
  /// escape sequences.
  LitString,
  "A single-line string literal (e.g., `\"hello\"`, `\"world\\n\"`).",
  "\"\\\"hello\\\"\"",
  "Malformed string like unterminated \"hello"
);

define_literal!(
  /// A multi-line string literal.
  ///
  /// Represents string literals that span multiple lines, often with special delimiters
  /// like triple quotes (`"""..."""` or `'''...'''`). Common in languages like Python,
  /// Kotlin, and Swift.
  LitMultilineString,
  "A multi-line string literal (e.g., `\"\"\"...\"\"\"`).",
  "\"\\\"\\\"\\\"multi\\nline\\\"\\\"\\\"\"",
  "Malformed multiline string"
);

define_literal!(
  /// A raw string literal.
  ///
  /// Represents string literals where escape sequences are not processed, often
  /// prefixed with `r` (e.g., Rust's `r"C:\path"`, Python's `r"\n stays literal"`).
  /// Useful for regular expressions and file paths.
  LitRawString,
  "A raw string literal without escape processing (e.g., `r\"C:\\path\"`).",
  "\"r\\\"C:\\\\path\\\"\"",
  "Malformed raw string"
);

// Character and byte literals
define_literal!(
  /// A character literal.
  ///
  /// Represents a single character enclosed in single quotes, such as `'a'`, `'\\n'`,
  /// or `'\\u{1F600}'`. May contain escape sequences for special characters.
  LitChar = char,
  "A character literal (e.g., `'a'`, `'\\n'`, `'\\u{1F600}'`).",
  "\"'a'\"",
  "Malformed char like unclosed 'a"
);

define_literal!(
  /// A byte literal.
  ///
  /// Represents a single byte value enclosed in single quotes with a `b` prefix,
  /// such as `b'a'`, `b'\\x7F'`, or `b'\\n'`. Used in languages like Rust for
  /// ASCII/byte manipulation.
  LitByte = u8,
  "A byte literal (e.g., `b'a'`, `b'\\x7F'`).",
  "\"b'a'\"",
  "Malformed byte literal"
);

define_literal!(
  /// A byte string literal.
  ///
  /// Represents a sequence of bytes enclosed in quotes with a `b` prefix, such as
  /// `b"bytes"`, `b"\\x48\\x65\\x6C\\x6C\\x6F"`. Used for binary data or ASCII strings.
  LitByteString,
  "A byte string literal (e.g., `b\"bytes\"`, `b\"\\x48\\x65\\x6C\\x6C\\x6F\"`).",
  "\"b\\\"bytes\\\"\"",
  "Malformed byte string"
);

// Boolean and null
define_literal!(
  /// A boolean literal.
  ///
  /// Represents boolean values `true` or `false`. The source string contains the
  /// actual keyword as it appears in source code.
  LitBool = bool,
  "A boolean literal (`true` or `false`).",
  "\"true\"",
  "Malformed boolean like \"tru\" or \"fals\""
);

define_literal!(
  /// A `true` literal.
  ///
  /// Represents boolean value `true`. The source string contains the
  /// actual keyword as it appears in source code.
  LitTrue = (),
  "A `true` literal.",
  "\"true\"",
  "Malformed `true` literal like \"tru\""
);

define_literal!(
  /// A `true` literal.
  ///
  /// Represents boolean value `true`. The source string contains the
  /// actual keyword as it appears in source code.
  LitFalse = (),
  "A `false` literal.",
  "\"false\"",
  "Malformed `false` literal like \"fals\""
);

define_literal!(
  /// A null/nil literal.
  ///
  /// Represents the null, nil, or None value in various programming languages.
  /// The source string contains the keyword as it appears (e.g., `null`, `nil`,
  /// `None`, `nullptr`).
  LitNull = (),
  "A null/nil literal (e.g., `null`, `nil`, `None`).",
  "\"null\"",
  "Malformed null literal"
);
