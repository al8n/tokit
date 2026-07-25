/// Defines a keyword.
///
/// # Examples
/// ```rust
/// use tokora::keyword;
///
/// keyword! {
///   (MyKeyword, "MY_KEYWORD", "my_keyword"),
///   (AnotherKeyword, "ANOTHER_KEYWORD", "another_keyword"),
/// }
/// ```
#[macro_export]
macro_rules! keyword {
  ($(
    $(#[$meta:meta])*
    (
      $name:ident, $syntax_tree_display: literal, $kw:literal
    )
  ),+$(,)?) => {
    paste::paste! {
      $(
        #[doc = "The `" $kw "` keyword"]
        $(#[$meta])*
        #[derive(::core::fmt::Debug, ::core::clone::Clone, ::core::marker::Copy, ::core::cmp::PartialEq, ::core::cmp::Eq, ::core::hash::Hash)]
        pub struct $name<S = $crate::__private::span::SimpleSpan, C = (), Lang: ?::core::marker::Sized = ()> {
          span: S,
          source: C,
          _lang: ::core::marker::PhantomData<Lang>,
        }

        impl $name<()> {
          #[doc = "A unit instance of the `" $kw "` keyword."]
          pub const UNIT: Self = {
            ::core::assert!(::core::mem::size_of::<Self>() == 0);
            ::core::assert!(::core::mem::align_of::<Self>() == 1);

            Self::new(())
          };

          #[doc = "Returns a unit instance of the `" $kw "` keyword."]
          #[inline(always)]
          pub const fn unit() -> Self {
            Self::UNIT
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> ::core::convert::AsRef<::core::primitive::str> for $name<S, C, Lang> {
          #[inline]
          fn as_ref(&self) -> &str {
            $kw
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> ::core::borrow::Borrow<str> for $name<S, C, Lang> {
          #[inline]
          fn borrow(&self) -> &str {
            ::core::convert::AsRef::<str>::as_ref(self)
          }
        }

        impl<S> $name<S> {
          /// Creates a new keyword.
          #[doc = "Creates a new `" $kw "` keyword."]
          #[inline(always)]
          pub const fn new(span: S) -> Self {
            Self { span, source: (), _lang: ::core::marker::PhantomData }
          }
        }

        impl<S, C> $name<S, C> {
          #[doc = "Creates a new `" $kw "` keyword with the given content."]
          #[inline(always)]
          pub const fn with_content(span: S, content: C) -> Self {
            Self { span, source: content, _lang: ::core::marker::PhantomData }
          }
        }

        impl<S, C, Lang: ?::core::marker::Sized> $name<S, C, Lang> {
          #[doc = "Changes the language type of the `" $kw "` keyword."]
          #[inline]
          pub fn change_language<N: ?::core::marker::Sized>(self) -> $name<S, C, N> {
            $name { span: self.span, source: self.source, _lang: ::core::marker::PhantomData }
          }

          #[doc = "Changes the language type of the `" $kw "` keyword."]
          #[inline]
          pub const fn change_language_const<N: ?::core::marker::Sized>(self) -> $name<S, C, N>
          where
            S: ::core::marker::Copy,
            C: ::core::marker::Copy,
          {
            $name { span: self.span, source: self.source, _lang: ::core::marker::PhantomData }
          }

          #[doc = "Returns the raw string literal of the `" $kw "` keyword."]
          #[inline]
          pub const fn raw() -> &'static ::core::primitive::str {
            $kw
          }

          #[doc = "Returns the raw string literal of the `" $kw "` keyword."]
          #[inline]
          pub const fn as_str(&self) -> &'static ::core::primitive::str {
            Self::raw()
          }

          #[doc = "Returns the span of the `" $kw "` keyword."]
          #[inline]
          pub const fn span(&self) -> &S {
            &self.span
          }

          #[doc = "Returns a reference to the content of the `" $kw "` keyword."]
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

        impl<'inp, T, S, C, Lang> $crate::__private::Check<T, ::core::primitive::bool> for $name<S, C, Lang>
        where
          T: $crate::__private::token::KeywordToken<'inp> + ?::core::marker::Sized + 'inp,
          Lang: ?::core::marker::Sized,
        {
          #[inline(always)]
          fn check(&self, target: &T) -> ::core::primitive::bool {
            ::core::cmp::PartialEq::eq(
              &$crate::__private::token::KeywordToken::keyword(target),
              &::core::option::Option::Some($kw),
            )
          }
        }

        impl $name {
          #[doc = "A parser that parses a token and returns a `" $name "` instance if it matches the `" $kw "` keyword."]
          ///
          /// If the function returns `Ok(ParseAttempt::Decline)`, it means the next token is not the
          /// `" $kw "` keyword, and promises no valid token is consumed.
          pub fn try_parse<'inp, L, Ctx, Cmpl>(
            inp: &mut $crate::InputRef<'inp, '_, L, Ctx, (), Cmpl>,
          ) -> ::core::result::Result<
            $crate::try_parse_input::ParseAttempt<$name<L::Span, ()>>,
            <Ctx::Emitter as $crate::Emitter<'inp, L>>::Error,
          >
          where
            L: $crate::Lexer<'inp>,
            L::Token: $crate::__private::token::KeywordToken<'inp>,
            Ctx: $crate::ParseContext<'inp, L>,
            Cmpl: $crate::input::SurfaceIncomplete<'inp, L, Ctx, ()>,
            <Ctx::Emitter as $crate::Emitter<'inp, L>>::Error:
              ::core::convert::From<$crate::error::UnexpectedEot<L::Offset>>,
          {
            Self::try_parse_of(inp)
          }

          #[doc = "A parser that parses a token and returns a `" $name "` instance if it matches the `" $kw "` keyword for a specific language."]
          ///
          /// If the function returns `Ok(ParseAttempt::Decline)`, it means the next token is not the
          /// `" $kw "` keyword, and promises no valid token is consumed.
          pub fn try_parse_of<'inp, L, Ctx, Lang: ?::core::marker::Sized, Cmpl>(
            inp: &mut $crate::InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
          ) -> ::core::result::Result<
            $crate::try_parse_input::ParseAttempt<$name<L::Span, (), Lang>>,
            <Ctx::Emitter as $crate::Emitter<'inp, L, Lang>>::Error,
          >
          where
            L: $crate::Lexer<'inp>,
            L::Token: $crate::__private::token::KeywordToken<'inp>,
            Ctx: $crate::ParseContext<'inp, L, Lang>,
            Cmpl: $crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
            <Ctx::Emitter as $crate::Emitter<'inp, L, Lang>>::Error:
              ::core::convert::From<$crate::error::UnexpectedEot<L::Offset, Lang>>,
          {
            inp
              .try_expect_or_stop(|t| ::core::cmp::PartialEq::eq(
                &$crate::__private::token::KeywordToken::keyword(t.into_data()),
                &::core::option::Option::Some($kw),
              ))
              .map(|res| res.map(|tok| $name::new(tok.into_span()).change_language()).into())
          }

          #[doc = "A parser that parses the `" $kw "` keyword, erroring when the next token is not that keyword."]
          ///
          /// Unlike [`try_parse`](Self::try_parse), an unexpected token is converted into an
          /// `UnexpectedToken` error carrying the found token, and end of input into an
          /// `UnexpectedEot` error.
          ///
          /// The error carries the found token; it does not carry an expected-token entry — the expected keyword is statically known at the call site.
          pub fn parse<'inp, L, Ctx, Cmpl>(
            inp: &mut $crate::InputRef<'inp, '_, L, Ctx, (), Cmpl>,
          ) -> ::core::result::Result<$name<L::Span, ()>, <Ctx::Emitter as $crate::Emitter<'inp, L>>::Error>
          where
            L: $crate::Lexer<'inp>,
            L::Token: $crate::__private::token::KeywordToken<'inp>,
            Ctx: $crate::ParseContext<'inp, L>,
            Cmpl: $crate::input::SurfaceIncomplete<'inp, L, Ctx, ()>,
            <Ctx::Emitter as $crate::Emitter<'inp, L>>::Error:
              ::core::convert::From<$crate::error::UnexpectedEot<L::Offset>>
              + ::core::convert::From<$crate::error::token::UnexpectedToken<'inp, L::Token, <L::Token as $crate::Token<'inp>>::Kind, L::Span>>,
          {
            Self::parse_of(inp)
          }

          #[doc = "A parser that parses the `" $kw "` keyword for a specific language, erroring when the next token is not that keyword."]
          ///
          /// Unlike [`try_parse_of`](Self::try_parse_of), an unexpected token is converted into an
          /// `UnexpectedToken` error carrying the found token, and end of input into an
          /// `UnexpectedEot` error.
          ///
          /// The error carries the found token; it does not carry an expected-token entry — the expected keyword is statically known at the call site.
          pub fn parse_of<'inp, L, Ctx, Lang: ?::core::marker::Sized, Cmpl>(
            inp: &mut $crate::InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
          ) -> ::core::result::Result<$name<L::Span, (), Lang>, <Ctx::Emitter as $crate::Emitter<'inp, L, Lang>>::Error>
          where
            L: $crate::Lexer<'inp>,
            L::Token: $crate::__private::token::KeywordToken<'inp>,
            Ctx: $crate::ParseContext<'inp, L, Lang>,
            Cmpl: $crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
            <Ctx::Emitter as $crate::Emitter<'inp, L, Lang>>::Error:
              ::core::convert::From<$crate::error::UnexpectedEot<L::Offset, Lang>>
              + ::core::convert::From<$crate::error::token::UnexpectedToken<'inp, L::Token, <L::Token as $crate::Token<'inp>>::Kind, L::Span, Lang>>,
          {
            match inp.next_or_stop()? {
              ::core::option::Option::Some(spanned) => {
                if ::core::cmp::PartialEq::eq(
                  &$crate::__private::token::KeywordToken::keyword(spanned.data()),
                  &::core::option::Option::Some($kw),
                ) {
                  ::core::result::Result::Ok($name::new(spanned.into_span()).change_language())
                } else {
                  let (span, tok) = spanned.into_components();
                  ::core::result::Result::Err(
                    $crate::error::token::UnexpectedToken::of(span).with_found(tok).into(),
                  )
                }
              }
              ::core::option::Option::None => ::core::result::Result::Err(
                $crate::error::UnexpectedEot::eot_of(
                  $crate::__private::span::Span::end(inp.span()),
                ).into(),
              ),
            }
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
          #[inline(always)]
          fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
            ::core::fmt::Write::write_str(f, $kw)
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

#[cfg(all(test, feature = "std", feature = "logos"))]
mod tests;
