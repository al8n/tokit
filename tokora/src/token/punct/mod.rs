use super::Token;
use crate::span::Spanned;

macro_rules! is_punctuator {
  ($this:ident($punct:ident, $($follow:ident),+$(,)?)) => {{
    paste::paste! {
      $this.[< is_ $punct >]() $(|| $this.[< is_ $follow >]())*
    }
  }};
}

macro_rules! define_punctuator_token_traits {
  (
    $(
      $(#[$meta:meta])*
      $name:ident::$punct:ident: $punct_char:literal
    ), +$(,)?
  ) => {
    paste::paste! {
      /// A trait for tokens that can classify punctuation without pattern matching on kinds.
      ///
      /// [`PunctuatorToken`] builds on [`Token`] to provide ergonomic helpers for recognizing
      /// common punctuation lexemes. This is useful when:
      ///
      /// - Building parsers that frequently branch on punctuation and benefit from readable predicates
      /// - Writing formatter or linter passes that need to treat punctuation uniformly regardless of kind names
      /// - Exposing a stable surface for downstream users so token-kind refactors do not cascade outward
      ///
      /// # Relationship to [`Token`]
      ///
      /// The base [`Token`] trait exposes [`Token::kind`], leaving higher-level classification to
      /// consumers. [`PunctuatorToken`] moves that logic into the token type itself, so downstream
      /// code can remain agnostic of `Kind` enums or their discriminants.
      ///
      /// # Covered ASCII Punctuation
      ///
      /// Every method maps to a single ASCII character and **returns `false` by default**—override only
      /// the ones that matter for your language, mapping them to your own token kinds. The provided
      /// predicates are:
      ///
      /// - Structural: `is_open_paren` `(`, `is_close_paren` `)`, `is_open_brace` `{`, `is_close_brace` `}`,
      ///   `is_open_bracket` `[`, `is_close_bracket` `]`
      /// - Separators: `is_comma` `,`, `is_dot` `.`, `is_colon` `:`, `is_semicolon` `;`
      /// - Quote markers: `is_double_quote` `"`, `is_apostrophe` `'`, `is_backtick` `` ` ``
      /// - Math / operators: `is_plus` `+`, `is_minus` `-`, `is_asterisk` `*`, `is_slash` `/`,
      ///   `is_backslash` `\`, `is_percent` `%`, `is_ampersand` `&`, `is_pipe` `|`, `is_caret` `^`,
      ///   `is_tilde` `~`, `is_underscore` `_`
      /// - Comparators: `is_lt` `<`, `is_gt` `>`, `is_equal` `=`
      /// - Misc punctuation: `is_exclamation` `!`, `is_question` `?`, `is_hash` `#`, `is_dollar` `$`,
      ///   `is_at` `@`
      ///
      /// ## Example
      ///
      /// ```rust
      /// use tokora::{Token, token::{PunctuatorToken, PunctuatorTokenExt}};
      /// use core::fmt;
      ///
      /// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
      /// enum MyTokenKind {
      ///     Dot,
      ///     Comma,
      ///     Colon,
      ///     SemiColon,
      /// }
      ///
      /// impl fmt::Display for MyTokenKind {
      ///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      ///         let name = match self {
      ///             Self::Dot => ".",
      ///             Self::Comma => ",",
      ///             Self::Colon => ":",
      ///             Self::SemiColon => ";",
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
      /// impl PunctuatorToken<'_> for MyToken {
      ///     fn dot() -> Option<Self::Kind> {
      ///         Some(MyTokenKind::Dot)
      ///     }
      ///
      ///     fn comma() -> Option<Self::Kind> {
      ///         Some(MyTokenKind::Comma)
      ///     }
      ///
      ///     fn colon() -> Option<Self::Kind> {
      ///         Some(MyTokenKind::Colon)
      ///     }
      ///
      ///     fn semicolon() -> Option<Self::Kind> {
      ///         Some(MyTokenKind::SemiColon)
      ///     }
      ///
      ///     // Unhandled punctuation can keep the default `None`.
      /// }
      ///
      /// let token = MyToken { kind: MyTokenKind::Dot };
      /// assert!(token.is_dot());
      /// ```
      pub trait PunctuatorToken<'a>: Token<'a> {
        $(
          #[doc = "Returns `Some(_)` when " $punct " (`" $punct_char "`) is one of kinds of the token."]
          $(#[$meta])*
          #[inline(always)]
          fn $punct() -> Option<Self::Kind> {
            None
          }
        )*
      }

      $(
        impl<'inp, T, S, C, Lang> $crate::__private::Check<T, ::core::primitive::bool> for $crate::punct::$name<S, C, Lang>
        where
          T: $crate::__private::token::PunctuatorToken<'inp> + ?::core::marker::Sized + 'inp,
          Lang: ?::core::marker::Sized,
        {
          #[inline(always)]
          fn check(&self, target: &T) -> ::core::primitive::bool {
            target.[< is_ $punct >]()
          }
        }
      )*

      /// Extension trait providing default implementations for punctuator token methods.
      pub trait PunctuatorTokenExt<'a>: PunctuatorToken<'a> {
        /// Returns `true` when the token is a punctuator.
        #[inline(always)]
        fn is_punctuator(&self) -> bool {
          is_punctuator!(self($($punct), +))
        }

        #[doc = "Returns `true` when the token is the less than punctuator (`<`)."]
        #[inline(always)]
        fn is_less_than(&self) -> bool {
          self.is_open_angle()
        }

        #[doc = "Returns `true` when the token is the greater than punctuator (`>`)."]
        #[inline(always)]
        fn is_greater_than(&self) -> bool {
          self.is_close_angle()
        }

        #[doc = "Returns `true` when the token is the bang punctuator (`!`)."]
        #[inline(always)]
        fn is_bang(&self) -> bool {
          self.is_exclamation()
        }

        #[doc = "Returns `true` when the token is the hyphen punctuator (`-`)."]
        #[inline(always)]
        fn is_hyphen(&self) -> bool {
          self.is_minus()
        }

        #[doc = "Returns `true` when the token is the thin arrow punctuator (`->`)."]
        #[inline(always)]
        fn is_thin_arrow(&self) -> bool {
          self.is_arrow()
        }

        #[doc = "Returns `true` when the token is the add assign punctuator (`+=`)."]
        #[inline(always)]
        fn is_add_assign(&self) -> bool {
          self.is_plus_equal()
        }

        #[doc = "Returns `true` when the token is the sub assign punctuator (`-=`)."]
        #[inline(always)]
        fn is_sub_assign(&self) -> bool {
          self.is_hyphen_equal()
        }

        #[doc = "Returns `true` when the token is the mul assign punctuator (`*=`)."]
        #[inline(always)]
        fn is_mul_assign(&self) -> bool {
          self.is_asterisk_equal()
        }

        #[doc = "Returns `true` when the token is the exponentiation assign punctuator (`**=`)."]
        #[inline(always)]
        fn is_exponentiation_assign(&self) -> bool {
          self.is_exponentiation_equal()
        }

        #[doc = "Returns `true` when the token is the div assign punctuator (`/=`)."]
        #[inline(always)]
        fn is_div_assign(&self) -> bool {
          self.is_slash_equal()
        }

        #[doc = "Returns `true` when the token is the and assign punctuator (`&=`)."]
        #[inline(always)]
        fn is_bitand_assign(&self) -> bool {
          self.is_ampersand_equal()
        }

        #[doc = "Returns `true` when the token is the or assign punctuator (`|=`)."]
        #[inline(always)]
        fn is_bitor_assign(&self) -> bool {
          self.is_pipe_equal()
        }

        #[doc = "Returns `true` when the token is the xor assign punctuator (`^=`)."]
        #[inline(always)]
        fn is_bitxor_assign(&self) -> bool {
          self.is_caret_equal()
        }

        #[doc = "Returns `true` when the token is the shl assign punctuator (`<<=`)."]
        #[inline(always)]
        fn is_shl_assign(&self) -> bool {
          self.is_shl_equal()
        }

        #[doc = "Returns `true` when the token is the shr assign punctuator (`>>=`)."]
        #[inline(always)]
        fn is_shr_assign(&self) -> bool {
          self.is_shr_equal()
        }

        #[doc = "Returns `true` when the token is the sar assign punctuator (`>>>=`)."]
        #[inline(always)]
        fn is_sar_assign(&self) -> bool {
          self.is_sar_equal()
        }

        $(
          #[doc = "Returns `true` when the token is the " $punct " punctuator (`" $punct_char "`)."]
          #[inline(always)]
          fn [< is_ $punct >](&self) -> bool {
            Self::$punct().is_some_and(|k| self.kind().eq(&k))
          }
        )*
      }

      impl<'a, T> PunctuatorTokenExt<'a> for T where T: PunctuatorToken<'a> {}

      /// Extension trait providing default implementations for punctuator token methods.
      pub trait SpannedPunctuatorToken<'a, L: crate::Lexer<'a>, Lang: ?Sized = ()>: Sized
      where
        L::Token: PunctuatorToken<'a>,
      {
        #[doc = "Returns `Some(_)` when the token is hyphen (`-`)."]
        #[inline(always)]
        fn expect_hyphen(self) -> Result<Spanned<L::Token, L::Span>, crate::error::token::UnexpectedToken<'a, L::Token, <L::Token as Token<'a>>::Kind, L::Span, Lang>>
        {
          self.expect_minus()
        }

        #[doc = "Returns `Some(_)` when the token is thin arrow (`->`)."]
        #[inline(always)]
        fn expect_thin_arrow(self) -> Result<Spanned<L::Token, L::Span>, crate::error::token::UnexpectedToken<'a, L::Token, <L::Token as Token<'a>>::Kind, L::Span, Lang>>
        {
          self.expect_arrow()
        }

        $(
          #[doc = "Returns `Some(_)` when the token is " $punct " (`" $punct_char "`)."]
          fn [< expect_ $punct >](self) -> Result<Spanned<L::Token, L::Span>, crate::error::token::UnexpectedToken<'a, L::Token, <L::Token as Token<'a>>::Kind, L::Span, Lang>>;
        )*
      }

      impl<'a, L, Lang: ?Sized> SpannedPunctuatorToken<'a, L, Lang> for crate::span::Spanned<L::Token, L::Span>
      where
        L: crate::Lexer<'a>,
        L::Token: PunctuatorToken<'a>,
      {
        $(
          #[doc = "Returns `Some(_)` when the token is " $punct " (`" $punct_char "`)."]
          #[inline(always)]
          fn [< expect_ $punct >](self) -> Result<Spanned<L::Token, L::Span>, crate::error::token::UnexpectedToken<'a, L::Token, <L::Token as Token<'a>>::Kind, L::Span, Lang>> {
            if self.data().[< is_ $punct >]() {
              Ok(self)
            } else {
              let (span, tok) = self.into_components();
              Err(crate::error::token::UnexpectedToken::expected_one(
                span,
                <L::Token>::$punct()
                  .expect(concat!("`", stringify!($punct), "` should not be `None` if `is_", stringify!($punct), "` is `true`")))
                  .with_found(tok)
              )
            }
          }
        )*
      }
    }
  };
}

define_punctuator_token_traits!(
  // Delimiters
  #[doc(alias = "less_than")]
  OpenAngle::open_angle: "<",
  #[doc(alias = "greater_than")]
  CloseAngle::close_angle: ">",
  OpenBrace::open_brace: "{",
  CloseBrace::close_brace: "}",
  OpenParen::open_paren: "(",
  CloseParen::close_paren: ")",
  OpenBracket::open_bracket: "[",
  CloseBracket::close_bracket: "]",

  // ASCII Punctuation
  At::at: "@",
  Asterisk::asterisk: "*",
  Ampersand::ampersand: "&",
  Apostrophe::apostrophe: "'",
  Backtick::backtick: "`",
  Backslash::backslash: "\\",
  Caret::caret: "^",
  Comma::comma: ",",
  Colon::colon: ":",
  Dot::dot: ".",
  Dollar::dollar: "$",
  DoubleQuote::double_quote: "\"",
  Equal::equal: "=",
  #[doc(alias = "bang")]
  Exclamation::exclamation: "!",
  Hash::hash: "#",
  #[doc(alias = "hyphen")]
  Hyphen::minus: "-",
  Pipe::pipe: "|",
  Plus::plus: "+",
  Percent::percent: "%",
  Question::question: "?",
  Slash::slash: "/",
  Semicolon::semicolon: ";",
  Tilde::tilde: "~",
  Underscore::underscore: "_",

  // Multi-character Punctuators
  #[doc(alias = "thin_arrow")]
  Arrow::arrow: "->",
  FatArrow::fat_arrow: "=>",
  #[doc(alias = "pipe_forward")]
  PipeArrow::pipe_arrow: "|>",

  // Equal related
  #[doc(alias = "colon_assign")]
  #[doc(alias = "short_declaration")]
  #[doc(alias = "colon_equals")]
  ColonEqual::colon_equal: ":=",
  LogicalEqual::logical_equal: "==",
  LogicalNotEqual::logical_not_equal: "!=",
  StrictEqual::strict_equal: "===",
  StrictNotEqual::strict_not_equal: "!==",
  LessThanOrEqual::less_than_or_equal: "<=",
  GreaterThanOrEqual::greater_than_or_equal: ">=",
  StrictLessThanOrEqual::strict_less_than_or_equal: "<==",
  StrictGreaterThanOrEqual::strict_greater_than_or_equal: ">==",

  #[doc(alias = "add_assign")]
  PlusEqual::plus_equal: "+=",
  #[doc(alias = "sub_assign")]
  HyphenEqual::hyphen_equal: "-=",
  #[doc(alias = "mul_assign")]
  AsteriskEqual::asterisk_equal: "*=",
  #[doc(alias = "exponentiation_assign")]
  ExponentiationEqual::exponentiation_equal: "**=",
  #[doc(alias = "div_assign")]
  SlashEqual::slash_equal: "/=",
  BackslashEqual::backslash_equal: "\\=",
  #[doc(alias = "mod_assign")]
  #[doc(alias = "percent_assign")]
  PercentEqual::percent_equal: "%=",

  #[doc(alias = "and_assign")]
  AmpersandEqual::ampersand_equal: "&=",
  #[doc(alias = "or_assign")]
  PipeEqual::pipe_equal: "|=",
  #[doc(alias = "xor_assign")]
  CaretEqual::caret_equal: "^=",

  #[doc(alias = "shl_assign")]
  ShlEqual::shl_equal: "<<=",
  #[doc(alias = "shr_assign")]
  ShrEqual::shr_equal: ">>=",
  #[doc(alias = "sar_assign")]
  SarEqual::sar_equal: ">>>=",

  ShiftLeft::shl: "<<",
  ShiftRight::shr: ">>",
  ShiftArithmeticRight::sar: ">>>",

  Increment::increment: "++",
  Decrement::decrement: "--",
  Exponentiation::exponentiation: "**",

  LogicalAnd::logical_and: "&&",
  LogicalOr::logical_or: "||",

  DoubleColon::double_colon: "::",
  Spread::spread: "...",
  #[doc(alias = "nullish_coalescing")]
  NullCoalesce::null_coalesce: "??",
  OptionalChain::optional_chain: "?.",

  // Trivia
  Tab::tab: "\t",
  Newline::newline: "\n",
  CarriageReturn::carriage_return: "\r",
  CarriageReturnNewline::crlf: "\r\n",
  Space::space: " ",
);

#[cfg(test)]
mod tests;
