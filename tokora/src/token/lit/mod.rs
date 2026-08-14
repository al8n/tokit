use super::*;

/// A trait for tokens that can classify literal tokens without exposing internal kinds.
///
/// [`LitToken`] augments [`Token`] with convenience predicates for common literal categories
/// (numbers, strings, booleans, etc.). This lets downstream code work with semantic literals
/// without matching on the token-kind enum directly.
///
/// # Usage
///
/// Every method **returns `false` by default**. Implementors override whichever literal kinds
/// their language supports, forwarding the checks to `self.kind()` or other internal data.
///
/// # Covered Literal Categories
///
/// - Numbers: `is_integer_literal`, `is_float_literal`, `is_decimal_literal`, `is_hexadecimal_literal`,
///   `is_octal_literal`, `is_binary_literal`, `is_hex_float_literal`
/// - Textual: `is_string_literal`, `is_inline_string_literal`, `is_multiline_string_literal`,
///   `is_raw_string_literal`, `is_char_literal`
/// - Byte-oriented: `is_byte_literal`, `is_byte_string_literal`
/// - Semantic markers: `is_boolean_literal`, `is_true_literal`, `is_false_literal`, `is_null_literal`
///
/// Override only what you need; everything else can keep the default `false`.
///
/// ## Example
///
/// ```rust
/// use tokora::{Token, token::LitToken};
/// use core::fmt;
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum MyTokenKind {
///     Integer,
///     Float,
///     String,
///     Boolean,
/// }
///
/// impl fmt::Display for MyTokenKind {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         let name = match self {
///             Self::Integer => "integer",
///             Self::Float => "float",
///             Self::String => "string",
///             Self::Boolean => "boolean",
///         };
///         f.write_str(name)
///     }
/// }
///
/// #[derive(Debug, Clone, PartialEq)]
/// struct MyToken {
///     kind: MyTokenKind,
/// }
///
/// impl Token<'_> for MyToken {
///     type Kind = MyTokenKind;
///     type Error = ();
///
///     fn kind(&self) -> Self::Kind {
///         self.kind
///     }
///
///     fn is_trivia(&self) -> bool {
///         false
///     }
/// }
///
/// impl LitToken<'_> for MyToken {
///     fn is_integer_literal(&self) -> bool {
///         matches!(self.kind, MyTokenKind::Integer)
///     }
///
///     fn is_float_literal(&self) -> bool {
///         matches!(self.kind, MyTokenKind::Float)
///     }
///
///     fn is_string_literal(&self) -> bool {
///         matches!(self.kind, MyTokenKind::String)
///     }
///
///     fn is_boolean_literal(&self) -> bool {
///         matches!(self.kind, MyTokenKind::Boolean)
///     }
/// }
///
/// let token = MyToken { kind: MyTokenKind::Integer };
/// assert!(token.is_integer_literal());
/// ```
pub trait LitToken<'a>: Token<'a> {
  /// Returns `true` if the token is any literal (number, string, boolean, etc.).
  #[inline(always)]
  fn is_literal(&self) -> bool {
    self.is_numeric_literal()
      || self.is_string_literal()
      || self.is_raw_string_literal()
      || self.is_char_literal()
      || self.is_byte_literal()
      || self.is_byte_string_literal()
      || self.is_boolean_literal()
      || self.is_null_literal()
  }

  /// Returns `true` when the token is any numeric literal.
  #[inline(always)]
  fn is_numeric_literal(&self) -> bool {
    self.is_integer_literal() || self.is_float_literal() || self.is_hex_float_literal()
  }

  /// Returns `true` when the token is an integer literal (e.g., binary, decimal, hex, octal).
  #[inline(always)]
  fn is_integer_literal(&self) -> bool {
    self.is_binary_literal()
      || self.is_decimal_literal()
      || self.is_hexadecimal_literal()
      || self.is_octal_literal()
  }

  /// Returns `true` when the token is a floating-point literal.
  #[inline(always)]
  fn is_float_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a base-10 integer literal.
  #[inline(always)]
  fn is_decimal_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a hexadecimal integer literal (e.g., `0xFF`).
  #[inline(always)]
  fn is_hexadecimal_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is an octal integer literal (e.g., `0o77`).
  #[inline(always)]
  fn is_octal_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a binary integer literal (e.g., `0b1010`).
  #[inline(always)]
  fn is_binary_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a hexadecimal floating-point literal (e.g., `0x1.fp3`).
  #[inline(always)]
  fn is_hex_float_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a **plain** string literal — inline
  /// ([`is_inline_string_literal`](Self::is_inline_string_literal)) or multi-line
  /// ([`is_multiline_string_literal`](Self::is_multiline_string_literal)) — and only those.
  ///
  /// Despite the general-sounding name, this predicate deliberately does **not** cover the
  /// sibling string *families*: a raw string
  /// ([`is_raw_string_literal`](Self::is_raw_string_literal)) and a byte string
  /// ([`is_byte_string_literal`](Self::is_byte_string_literal)) are their own classes, ORed in
  /// separately by [`is_literal`](Self::is_literal). The split matters to consumers that
  /// dispatch on escape processing: a plain string is the *escape-processed* quoted form,
  /// while a raw string is taken verbatim and a byte string is not text at all.
  #[inline(always)]
  fn is_string_literal(&self) -> bool {
    self.is_inline_string_literal() || self.is_multiline_string_literal()
  }

  /// Returns `true` when the token is a single-line/inline string literal.
  #[inline(always)]
  fn is_inline_string_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a multi-line string literal.
  #[inline(always)]
  fn is_multiline_string_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a raw string literal.
  #[inline(always)]
  fn is_raw_string_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a character literal (e.g., `'a'`).
  #[inline(always)]
  fn is_char_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a byte literal (e.g., `b'a'`).
  #[inline(always)]
  fn is_byte_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a byte-string literal (e.g., `b"..."`).
  #[inline(always)]
  fn is_byte_string_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a boolean literal (`true`/`false`).
  #[inline(always)]
  fn is_boolean_literal(&self) -> bool {
    self.is_true_literal() || self.is_false_literal()
  }

  /// Returns `true` when the token is the `true` literal.
  #[inline(always)]
  fn is_true_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is the `false` literal.
  #[inline(always)]
  fn is_false_literal(&self) -> bool {
    false
  }

  /// Returns `true` when the token is a null/nil literal.
  #[inline(always)]
  fn is_null_literal(&self) -> bool {
    false
  }
}

impl<'a, T> LitToken<'a> for &'a T
where
  T: LitToken<'a>,
{
  #[inline(always)]
  fn is_literal(&self) -> bool {
    (**self).is_literal()
  }

  #[inline(always)]
  fn is_numeric_literal(&self) -> bool {
    (**self).is_numeric_literal()
  }

  #[inline(always)]
  fn is_integer_literal(&self) -> bool {
    (**self).is_integer_literal()
  }

  #[inline(always)]
  fn is_float_literal(&self) -> bool {
    (**self).is_float_literal()
  }

  #[inline(always)]
  fn is_decimal_literal(&self) -> bool {
    (**self).is_decimal_literal()
  }

  #[inline(always)]
  fn is_hexadecimal_literal(&self) -> bool {
    (**self).is_hexadecimal_literal()
  }

  #[inline(always)]
  fn is_octal_literal(&self) -> bool {
    (**self).is_octal_literal()
  }

  #[inline(always)]
  fn is_binary_literal(&self) -> bool {
    (**self).is_binary_literal()
  }

  #[inline(always)]
  fn is_hex_float_literal(&self) -> bool {
    (**self).is_hex_float_literal()
  }

  #[inline(always)]
  fn is_string_literal(&self) -> bool {
    (**self).is_string_literal()
  }

  #[inline(always)]
  fn is_inline_string_literal(&self) -> bool {
    (**self).is_inline_string_literal()
  }

  #[inline(always)]
  fn is_multiline_string_literal(&self) -> bool {
    (**self).is_multiline_string_literal()
  }

  #[inline(always)]
  fn is_raw_string_literal(&self) -> bool {
    (**self).is_raw_string_literal()
  }

  #[inline(always)]
  fn is_char_literal(&self) -> bool {
    (**self).is_char_literal()
  }

  #[inline(always)]
  fn is_byte_literal(&self) -> bool {
    (**self).is_byte_literal()
  }

  #[inline(always)]
  fn is_byte_string_literal(&self) -> bool {
    (**self).is_byte_string_literal()
  }

  #[inline(always)]
  fn is_boolean_literal(&self) -> bool {
    (**self).is_boolean_literal()
  }

  #[inline(always)]
  fn is_true_literal(&self) -> bool {
    (**self).is_true_literal()
  }

  #[inline(always)]
  fn is_false_literal(&self) -> bool {
    (**self).is_false_literal()
  }

  #[inline(always)]
  fn is_null_literal(&self) -> bool {
    (**self).is_null_literal()
  }
}

#[cfg(test)]
mod tests;
