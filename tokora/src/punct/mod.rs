use crate::{Lexer, Token, error::token::UnexpectedToken, span::Spanned, utils::CowStr};

/// The carriage return newline (`\r\n`) punctuator.
pub type Crnl<S, C = (), Lang = ()> = CarriageReturnNewline<S, C, Lang>;

/// Defines the punctuators.
///
/// # Examples
/// ```rust
/// use tokora::punctuator;
///
/// punctuator! {
///   (LAngle, "L_ANGLE", "<"),
///   (RAngle, "R_ANGLE", ">"),
/// }
/// ```
#[macro_export]
macro_rules! punctuator {
  ($(
    $(#[$attr:meta])*
    ($name:ident, $syntax_tree_display: literal, $punct:literal)),+$(,)?
  ) => {
    paste::paste! {
      $(
        $(#[$attr])*
        #[doc = "The `" $punct "` punctuator"]
        #[derive(::core::fmt::Debug, ::core::clone::Clone, ::core::marker::Copy, ::core::cmp::PartialEq, ::core::cmp::Eq, ::core::hash::Hash)]
        pub struct $name<S = (), C = (), Lang: ?::core::marker::Sized = ()> {
          span: S,
          source: C,
          _lang: ::core::marker::PhantomData<Lang>,
        }

        impl $name<()> {
          #[doc = "A unit instance of the `" $punct "` punctuator."]
          pub const UNIT: Self = {
            ::core::assert!(::core::mem::size_of::<Self>() == 0);
            ::core::assert!(::core::mem::align_of::<Self>() == 1);

            Self::new(())
          };

          #[doc = "Returns a unit instance of the `" $punct "` punctuator."]
          #[inline]
          pub const fn unit() -> Self {
            Self::UNIT
          }
        }

        impl $name {
          #[doc = "Returns the raw string literal of the `" $punct "` punctuator."]
          #[inline]
          pub const fn raw() -> &'static ::core::primitive::str {
            $punct
          }
        }

        impl<S> $name<S> {
          /// Creates a new punctuator with the given span.
          #[inline]
          pub const fn new(span: S) -> Self {
            Self { span, source: (), _lang: ::core::marker::PhantomData }
          }
        }

        impl<S, C> $name<S, C> {
          #[doc = "Creates a new `" $punct "` punctuator with the given span and content."]
          #[inline]
          pub const fn with_content(span: S, content: C) -> Self {
            Self { span, source: content, _lang: ::core::marker::PhantomData }
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> $name<S, C, Lang> {
          #[doc = "Changes the language type of the `" $punct "` punctuator."]
          #[inline]
          pub fn change_language<N: ?::core::marker::Sized>(self) -> $name<S, C, N> {
            $name { span: self.span, source: self.source, _lang: ::core::marker::PhantomData }
          }

          #[doc = "Changes the language type of the `" $punct "` punctuator."]
          #[inline]
          pub const fn change_language_const<N: ?::core::marker::Sized>(self) -> $name<S, C, N>
          where
            S: ::core::marker::Copy,
            C: ::core::marker::Copy,
          {
            $name { span: self.span, source: self.source, _lang: ::core::marker::PhantomData }
          }

          #[doc = "Returns the raw string literal of the `" $punct "` punctuator."]
          #[inline]
          pub const fn as_str(&self) -> &'static ::core::primitive::str {
            <$name>::raw()
          }

          #[doc = "Returns the span of the `" $punct "` punctuator."]
          #[inline]
          pub const fn span(&self) -> &S {
            &self.span
          }

          #[doc = "Returns a reference to the content of the `" $punct "` punctuator."]
          #[inline]
          pub const fn content(&self) -> &C {
            &self.source
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> ::core::cmp::PartialEq<::core::primitive::str> for $name<S, C, Lang> {
          #[inline]
          fn eq(&self, other: &::core::primitive::str) -> bool {
            self.as_str().eq(other)
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> ::core::cmp::PartialOrd<::core::primitive::str> for $name<S, C, Lang> {
          #[inline]
          fn partial_cmp(&self, other: &::core::primitive::str) -> ::core::option::Option<::core::cmp::Ordering> {
            self.as_str().partial_cmp(other)
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> ::core::cmp::PartialEq<$name<S, C, Lang>> for ::core::primitive::str {
          #[inline]
          fn eq(&self, other: &$name<S, C, Lang>) -> bool {
            self.eq(other.as_str())
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> ::core::cmp::PartialOrd<$name<S, C, Lang>> for ::core::primitive::str {
          #[inline]
          fn partial_cmp(&self, other: &$name<S, C, Lang>) -> ::core::option::Option<::core::cmp::Ordering> {
            self.partial_cmp(other.as_str())
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> ::core::borrow::Borrow<::core::primitive::str> for $name<S, C, Lang> {
          #[inline]
          fn borrow(&self) -> &::core::primitive::str {
            self.as_str()
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> ::core::convert::AsRef<::core::primitive::str> for $name<S, C, Lang> {
          #[inline]
          fn as_ref(&self) -> &::core::primitive::str {
            self.as_str()
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> $crate::__private::span::AsSpan<S> for $name<S, C, Lang> {
          #[inline]
          fn as_span(&self) -> &S {
            self.span()
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> $crate::__private::span::IntoSpan<S> for $name<S, C, Lang> {
          #[inline]
          fn into_span(self) -> S {
            self.span
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> $crate::__private::utils::IntoComponents for $name<S, C, Lang> {
          type Components = (S, C);

          #[inline]
          fn into_components(self) -> Self::Components {
            (self.span, self.source)
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> ::core::fmt::Display for $name<S, C, Lang> {
          #[inline]
          fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
            ::core::fmt::Display::fmt($punct, f)
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> $crate::__private::utils::human_display::DisplayHuman for $name<S, C, Lang> {
          #[inline]
          fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
            ::core::fmt::Display::fmt(self, f)
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> $crate::__private::utils::sdl_display::DisplayCompact for $name<S, C, Lang> {
          type Options = ();

          #[inline]
          fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>, _: &Self::Options) -> ::core::fmt::Result {
            ::core::fmt::Display::fmt(self, f)
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> $crate::__private::utils::sdl_display::DisplayPretty for $name<S, C, Lang> {
          type Options = ();

          #[inline]
          fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>, _: &Self::Options) -> ::core::fmt::Result {
            ::core::fmt::Display::fmt(self, f)
          }
        }
      )*
    }
  };
}

punctuator! {
  // Delimiters
  #[doc(alias = "LessThan")]
  (OpenAngle, "OPEN_ANGLE", "<"),
  #[doc(alias = "GreaterThan")]
  (CloseAngle, "CLOSE_ANGLE", ">"),
  #[doc = "The angle-bracket (`<>`) delimiter pair. Its `Delimiter` impl is provided for symmetry with the other builtin delimiter pairs and is currently exercised only through the error-type aliases."]
  (Angle, "ANGLE", "<>"),
  (OpenBrace, "OPEN_BRACE", "{"),
  (CloseBrace, "CLOSE_BRACE", "}"),
  (OpenParen, "OPEN_PAREN", "("),
  (CloseParen, "CLOSE_PAREN", ")"),
  (Paren, "PAREN", "()"),
  (Brace, "BRACE", "{}"),
  (Bracket, "BRACKET", "[]"),
  (OpenBracket, "OPEN_BRACKET", "["),
  (CloseBracket, "CLOSE_BRACKET", "]"),

  // ASCII Punctuation
  (At, "AT", "@"),
  (Asterisk, "ASTERISK", "*"),
  #[doc(alias = "And")]
  (Ampersand, "AMPERSAND", "&"),
  (Apostrophe, "APOSTROPHE", "'"),
  (Backtick, "BACKTICK", "`"),
  (Backslash, "BACKSLASH", "\\"),
  #[doc(alias = "BitXor")]
  (Caret, "CARET", "^"),
  (Comma, "COMMA", ","),
  (Colon, "COLON", ":"),
  (Dot, "DOT", "."),
  (Dollar, "DOLLAR", "$"),
  (DoubleQuote, "DOUBLE_QUOTE", "\""),
  (Equal, "EQUAL", "="),
  #[doc(alias = "Bang")]
  (Exclamation, "EXCLAMATION", "!"),
  (Hash, "HASH", "#"),
  #[doc(alias = "Minus")]
  (Hyphen, "HYPHEN", "-"),
  (Pipe, "PIPE", "|"),
  (Plus, "PLUS", "+"),
  (Percent, "PERCENT", "%"),
  (Question, "QUESTION", "?"),
  (Slash, "SLASH", "/"),
  (Semicolon, "SEMICOLON", ";"),
  (Tilde, "TILDE", "~"),
  (Underscore, "UNDERSCORE", "_"),

  // Multi-character Punctuators
  #[doc(alias = "ThinArrow")]
  (Arrow, "ARROW", "->"),
  (FatArrow, "FAT_ARROW", "=>"),
  #[doc(alias = "PipeForward")]
  (PipeArrow, "PIPE_ARROW", "|>"),

  // Equal related
  #[doc(alias = "ColonAssign")]
  #[doc(alias = "ShortDeclaration")]
  #[doc(alias = "ColonEquals")]
  (ColonEqual, "COLON_EQUAL", ":="),
  (LogicalEqual, "LOGICAL_EQUAL", "=="),
  (LogicalNotEqual, "LOGICAL_NOT_EQUAL", "!="),
  (StrictEqual, "STRICT_EQUAL", "==="),
  (StrictNotEqual, "STRICT_NOT_EQUAL", "!=="),
  (LessThanOrEqual, "LESS_THAN_OR_EQUAL", "<="),
  (GreaterThanOrEqual, "GREATER_THAN_OR_EQUAL", ">="),
  (StrictLessThanOrEqual, "STRICT_LESS_THAN_OR_EQUAL", "<=="),
  (StrictGreaterThanOrEqual, "STRICT_GREATER_THAN_OR_EQUAL", ">=="),

  #[doc(alias = "AddAssign")]
  (PlusEqual, "PLUS_EQUAL", "+="),
  #[doc(alias = "SubAssign")]
  (HyphenEqual, "HYPHEN_EQUAL", "-="),
  #[doc(alias = "MulAssign")]
  (AsteriskEqual, "ASTERISK_EQUAL", "*="),
  #[doc(alias = "ExponentiationAssign")]
  (ExponentiationEqual, "EXPONENTIATION_EQUAL", "**="),
  #[doc(alias = "DivAssign")]
  (SlashEqual, "SLASH_EQUAL", "/="),
  (BackslashEqual, "BACKSLASH_EQUAL", "\\="),
  #[doc(alias = "RemAssign")]
  (PercentEqual, "PERCENT_EQUAL", "%="),
  #[doc(alias = "AndAssign")]
  (AmpersandEqual, "AMPERSAND_EQUAL", "&="),
  #[doc(alias = "OrAssign")]
  (PipeEqual, "PIPE_EQUAL", "|="),
  #[doc(alias = "XorAssign")]
  (CaretEqual, "CARET_EQUAL", "^="),
  #[doc(alias = "ShlAssign")]
  (ShlEqual, "SHL_EQUAL", "<<="),
  #[doc(alias = "ShrAssign")]
  (ShrEqual, "SHR_EQUAL", ">>="),
  #[doc(alias = "SarAssign")]
  (SarEqual, "SAR_EQUAL", ">>>="),


  (ShiftLeft, "SHIFT_LEFT", "<<"),
  (ShiftRight, "SHIFT_RIGHT", ">>"),
  (ShiftArithmeticRight, "SHIFT_ARITHMETIC_RIGHT", ">>>"),

  (Increment, "INCREMENT", "++"),
  (Decrement, "DECREMENT", "--"),
  (Exponentiation, "EXPONENTIATION", "**"),

  (LogicalAnd, "LOGICAL_AND", "&&"),
  (LogicalOr, "LOGICAL_OR", "||"),

  (DoubleColon, "DOUBLE_COLON", "::"),

  (Spread, "SPREAD", "..."),
  #[doc(alias = "NullishCoalescing")]
  (NullCoalesce, "NULL_COALESCE", "??"),
  (OptionalChain, "OPTIONAL_CHAIN", "?."),

  // Trivia
  (Tab, "TAB", "\t"),
  (Newline, "NEWLINE", "\n"),
  (CarriageReturn, "CARRIAGE_RETURN", "\r"),
  (CarriageReturnNewline, "CARRIAGE_RETURN_NEWLINE", "\r\n"),
  (Space, "SPACE", " "),
  (Trivia, "TRIVIA", "any trivia characters"),
}

/// A trait for any punctuator.
///
/// # The language brand is a fence
///
/// Every built-in marker carries its own brand (`Comma<S, C, Lang>`), and the blanket impls
/// generated for them name that brand and the context language as the **same** parameter. A
/// marker branded for one grammar is therefore not a punctuator of another: a separator
/// copy-pasted out of a sibling dialect fails to compile instead of quietly type-checking.
///
/// This costs the fluent family nothing. `separated_by_comma`, `separated_by_comma_while` and
/// their siblings stay reachable at a branded language because the macro that generates them
/// instantiates the marker at the *caller's* `Lang` — `Comma<(), (), Lang>` — rather than at a
/// bare `Comma<(), (), ()>` that a widened impl would have to accept.
///
/// The next two examples are the record. They differ in exactly one token, and only the one
/// whose marker agrees with the grammar asking for it compiles.
///
/// A marker branded for another language — **does not compile**:
///
/// ```rust,compile_fail,E0277
/// use tokora::{
///   Lexer, Token,
///   punct::{Comma, Punctuator},
/// };
///
/// struct LangA;
/// struct LangB;
///
/// fn assert_punct<'inp, L, P, Lang>()
/// where
///   L: Lexer<'inp>,
///   P: Punctuator<'inp, L, Lang>,
/// {
/// }
///
/// fn probe<'inp, L>()
/// where
///   L: Lexer<'inp>,
///   <L::Token as Token<'inp>>::Kind: From<Comma<(), (), ()>>,
/// {
///   assert_punct::<L, Comma<(), (), LangA>, LangA>();
///   assert_punct::<L, Comma<(), (), LangA>, LangB>();
/// }
/// ```
///
/// The same probe with each marker branded for its own grammar — compiles:
///
/// ```rust
/// use tokora::{
///   Lexer, Token,
///   punct::{Comma, Punctuator},
/// };
///
/// struct LangA;
/// struct LangB;
///
/// fn assert_punct<'inp, L, P, Lang>()
/// where
///   L: Lexer<'inp>,
///   P: Punctuator<'inp, L, Lang>,
/// {
/// }
///
/// fn probe<'inp, L>()
/// where
///   L: Lexer<'inp>,
///   <L::Token as Token<'inp>>::Kind: From<Comma<(), (), ()>>,
/// {
///   assert_punct::<L, Comma<(), (), LangA>, LangA>();
///   assert_punct::<L, Comma<(), (), LangB>, LangB>();
/// }
/// ```
///
/// The pair is also the tripwire for the day the fence is reopened: widen the impl back to two
/// independent language parameters and the first example starts compiling, failing its
/// `compile_fail`.
pub trait Punctuator<'inp, L, Lang: ?Sized = ()> {
  /// Returns the name of the punctuator.
  fn name() -> CowStr;

  /// Returns the description of the punctuator.
  #[inline]
  fn description() -> Option<CowStr> {
    None
  }

  /// Returns the kind of the punctuator.
  fn kind() -> <L::Token as Token<'inp>>::Kind
  where
    L: Lexer<'inp>;

  /// Evaluates whether the given token kind matches the punctuator's kind.
  #[inline]
  fn eval(knd: &<L::Token as Token<'inp>>::Kind) -> bool
  where
    L: Lexer<'inp>,
  {
    Self::kind().eq(knd)
  }

  /// Creates an `UnexpectedToken` error for the punctuator.
  #[inline]
  fn unexpected_token(
    tok: Spanned<L::Token, L::Span>,
  ) -> UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>
  where
    L: Lexer<'inp>,
  {
    let (span, tok) = tok.into_components();
    UnexpectedToken::expected_one(span, Self::kind()).with_found(tok)
  }
}

macro_rules! impl_deref {
  (@impl<$ty:ty>) => {
    #[inline]
    fn name() -> CowStr {
      <$ty>::name()
    }

    #[inline]
    fn description() -> Option<CowStr> {
      <$ty>::description()
    }

    #[inline]
    fn kind() -> <L::Token as Token<'inp>>::Kind
    where
      L: Lexer<'inp>,
    {
      <$ty>::kind()
    }

    #[inline]
    fn eval(knd: &<<L>::Token as Token<'inp>>::Kind) -> bool
    where
      L: Lexer<'inp>,
    {
      <$ty>::eval(knd)
    }

    #[inline]
    fn unexpected_token(
      tok: Spanned<<L>::Token, <L>::Span>,
    ) -> UnexpectedToken<'inp, <L>::Token, <<L>::Token as Token<'inp>>::Kind, <L>::Span, Lang>
    where
      L: Lexer<'inp>,
    {
      <$ty>::unexpected_token(tok)
    }
  };
}

impl<'inp, L, Lang: ?Sized, P> Punctuator<'inp, L, Lang> for &P
where
  L: Lexer<'inp>,
  P: Punctuator<'inp, L, Lang>,
{
  impl_deref!(@impl<P>);
}

impl<'inp, L, Lang: ?Sized, P> Punctuator<'inp, L, Lang> for &mut P
where
  L: Lexer<'inp>,
  P: Punctuator<'inp, L, Lang>,
{
  impl_deref!(@impl<P>);
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
