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
//! ```rust
//! # struct YulLang;
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
//! let text = YulString::new(SimpleSpan::new(5, 12), "\"hello\"");
//!
//! assert_eq!(generic.data_ref(), &"42");
//! assert_eq!(num.data_ref(), &"42");
//! assert_eq!(text.span(), SimpleSpan::new(5, 12));
//! ```
//!
//! ## Owned Literals
//!
//! ```rust
//! # struct MyLang;
//! # let span = tokora::SimpleSpan::new(0, 2);
//! # let source = "42";
//! use tokora::{SimpleSpan, types::LitDecimal};
//!
//! // Store literals in AST nodes
//! type OwnedDecimal = LitDecimal<String, SimpleSpan, MyLang>;
//!
//! let lit = OwnedDecimal::new(span, source.to_string());
//! assert_eq!(lit.data_ref(), "42");
//! ```
//!
//! # Error Recovery
//!
//! All literal types implement [`ErrorNode`] when `S: ErrorNode`:
//!
//! ```rust
//! # struct YulLang;
//! # let span = tokora::SimpleSpan::new(0, 2);
//! use tokora::{SimpleSpan, error::ErrorNode, types::{LitDecimal, recovery::RecoveryState}};
//!
//! // Create placeholder for malformed literal
//! let bad_lit = LitDecimal::<&str, SimpleSpan, YulLang>::error(span);
//! assert!(bad_lit.is_error());
//!
//! // Create placeholder for missing literal
//! let missing_lit = LitDecimal::<&str, SimpleSpan, YulLang>::missing(span);
//! assert!(missing_lit.is_missing());
//! ```
//!
//! Which of the three states a literal is in is read through the `RecoveryState` trait, which
//! must be in scope, and never from the data. There is no inherent accessor: see the trait for
//! why an inherent one cannot fail loudly. A placeholder's data is whatever `D::error` /
//! `D::missing` produced — for `&str` the literal `"<error>"`, a value a caller can also spell
//! by hand — and it is mutable through `data_mut`, so it reports what the node says rather
//! than whether the parser found one.

use core::marker::PhantomData;

use super::recovery::Status;
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
      /// ```rust
      #[doc = "use tokora::types::" $name ";"]
      /// use tokora::{SimpleSpan, error::ErrorNode, types::recovery::RecoveryState};
      /// # struct MyLang;
      /// # let span = SimpleSpan::new(0, 4);
      ///
      #[doc = "// " $example_desc]
      #[doc = "let bad_lit = " $name "::<&str, SimpleSpan, MyLang>::error(span);"]
      /// assert!(bad_lit.is_error());
      /// ```
      // `PartialEq`/`Eq` are derived while the other four are written out. A derive constrains
      // every type parameter, which is why the other four are hand-written — none of them reads
      // `Lang`. These two cannot join them: the derive is what emits `StructuralPartialEq`,
      // without which a `const` of one of these seventeen types cannot appear in a `match`
      // pattern, and that is not observable by checking rendered output. The residual is that
      // comparing or matching one still needs `Lang` to be `PartialEq + Eq`, as 0.9 required.
      #[derive(PartialEq, Eq)]
      pub struct $name<
        D: ?::core::marker::Sized $( = $default)?,
        Span = $crate::__private::span::SimpleSpan,
        Lang: ?::core::marker::Sized = (),
      > {
        _lang: PhantomData<Lang>,
        status: Status,
        span: Span,
        data: D,
      }

      // Written out rather than derived, and the only thing that changes is the bound list. A
      // derive constrains **every** type parameter, so `#[derive(Debug)]` here emitted
      // `Lang: Debug` — a requirement on a marker that is never printed, compared or hashed,
      // since `PhantomData<T>` implements all six for any `T` unconditionally. Seventeen literal
      // types were unusable with an underived language marker for a reason that does not exist.
      //
      // Rendering, comparison order and hashed bytes are unchanged: every field is still visited
      // in declaration order, the possibly-unsized one still goes in behind a second reference
      // the way the derive passes it, and `PhantomData` hashes nothing.
      impl<D, Span, Lang> ::core::fmt::Debug for $name<D, Span, Lang>
      where
        D: ::core::fmt::Debug + ?::core::marker::Sized,
        Span: ::core::fmt::Debug,
        Lang: ?::core::marker::Sized,
      {
        fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
          f.debug_struct(::core::stringify!($name))
            .field("_lang", &&self._lang)
            .field("status", &&self.status)
            .field("span", &&self.span)
            .field("data", &&self.data)
            .finish()
        }
      }

      impl<D, Span, Lang> ::core::clone::Clone for $name<D, Span, Lang>
      where
        D: ::core::clone::Clone,
        Span: ::core::clone::Clone,
        Lang: ?::core::marker::Sized,
      {
        #[inline]
        fn clone(&self) -> Self {
          Self {
            _lang: PhantomData,
            status: self.status,
            span: ::core::clone::Clone::clone(&self.span),
            data: ::core::clone::Clone::clone(&self.data),
          }
        }
      }

      impl<D, Span, Lang> ::core::marker::Copy for $name<D, Span, Lang>
      where
        D: ::core::marker::Copy,
        Span: ::core::marker::Copy,
        Lang: ?::core::marker::Sized,
      {
      }



      impl<D, Span, Lang> ::core::hash::Hash for $name<D, Span, Lang>
      where
        D: ::core::hash::Hash + ?::core::marker::Sized,
        Span: ::core::hash::Hash,
        Lang: ?::core::marker::Sized,
      {
        #[inline]
        fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) {
          ::core::hash::Hash::hash(&self.status, state);
          ::core::hash::Hash::hash(&self.span, state);
          ::core::hash::Hash::hash(&self.data, state);
        }
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
      /// The span, the data, **and the recovery status** — every field this type holds beyond
      /// the zero-sized language marker, which the rebuild names in its own type.
      ///
      /// The status is here because this trait promises a complete decomposition and because
      /// `FromComponents` is the inverse: without it in both, a consumer who took a literal apart
      /// and put it back together would have to rebuild through `new`, which always declares the
      /// result valid.
      type Components = $crate::types::recovery::Components<Span, D>;

      #[inline(always)]
      fn into_components(self) -> Self::Components {
        let Self { _lang, status, span, data } = self;

        $crate::types::recovery::Components { span, payload: data, status }
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
        Self::with_status(span, data, Status::Valid)
      }

      /// The status-setting constructor, crate-internal. Public construction with a chosen
      /// status goes through `FromComponents`; see that trait for why an inherent
      /// `with_status` could be re-targeted by a type-directed argument.
      #[inline(always)]
      const fn with_status(span: Span, data: D, status: Status) -> Self {
        Self {
          span,
          data,
          status,
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

    impl<D, Span, Lang: ?::core::marker::Sized> $crate::types::recovery::FromComponents
      for $name<D, Span, Lang>
    {
      #[inline(always)]
      fn from_components(components: Self::Components) -> Self {
        let $crate::types::recovery::Components { span, payload, status } = components;

        Self::with_status(span, payload, status)
      }
    }

    impl<D: ?::core::marker::Sized, Span, Lang: ?::core::marker::Sized> $crate::types::recovery::RecoveryState
      for $name<D, Span, Lang>
    {
      #[inline(always)]
      fn status(&self) -> Status {
        self.status
      }
    }

    impl<D, Span, Lang: ?::core::marker::Sized> ErrorNode<Span> for $name<D, Span, Lang>
    where
      D: ErrorNode<Span>,
      Span: Clone,
    {
      /// Creates a placeholder literal for **malformed content**.
      ///
      /// The result reports itself through `is_error`; the data is a placeholder, not the
      /// channel.
      #[inline(always)]
      fn error(span: Span) -> Self {
        Self::with_status(span.clone(), D::error(span), Status::Error)
      }

      /// Creates a placeholder literal for **missing required content**.
      ///
      /// The result reports itself through `is_missing`; the data is a placeholder, not the
      /// channel.
      #[inline(always)]
      fn missing(span: Span) -> Self {
        Self::with_status(span.clone(), D::missing(span), Status::Missing)
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
