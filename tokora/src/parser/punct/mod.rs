use super::*;

use crate::{
  error::UnexpectedEot,
  punct::*,
  token::{PunctuatorToken, PunctuatorTokenExt},
  try_parse_input::ParseAttempt,
  utils::CowStr,
};

macro_rules! define_parsers {
  ($($name:ident::$kind:ident::$punct_char:literal),+$(,)?) => {
    paste::paste! {
      $(
        impl $name {
          #[doc = "A parser that parses a token and returns a `" $name "` instance if it matches."]
          ///
          /// If the function returns `Ok(ParseAttempt::Decline)`, it means the next token does not match,
          /// and promises no valid token is consumed; a terminal scanner stop is an error, never a
          /// `Decline`.
          ///
          /// `Lang` is read off the input, so an unbranded grammar writes the same call as a
          /// branded one.
          pub fn try_parse<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
            inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
          ) -> Result<ParseAttempt<$name<L::Span, (), Lang>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
          where
            L: Lexer<'inp>,
            L::Token: PunctuatorToken<'inp>,
            Ctx: ParseContext<'inp, L, Lang>,
            Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
            <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
          {
            inp.try_expect_or_stop(|t| t.data.[<is_ $kind>]()).map(|res| res.map(|tok| $name::new(tok.into_span()).change_language()).into())
          }

          #[doc = "A parser that parses a token and returns a `" $name "` instance if it matches."]
          ///
          /// `Lang` is read off the input, so an unbranded grammar writes the same call as a
          /// branded one.
          pub fn parse<'inp, L, Ctx, Lang, Cmpl>(
            inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
          ) -> Result<$name<L::Span, (), Lang>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
          where
            L: Lexer<'inp>,
            L::Token: PunctuatorToken<'inp>,
            Ctx: ParseContext<'inp, L, Lang>,
            Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
            <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>> +
            From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>>,
            Lang: ?Sized,
          {
            inp.[<expect_ $kind>]().map(|tok| $name::new(tok.into_span()).change_language())
          }
        }

        impl<'inp, L, S, C, MarkerLang: ?Sized, Lang: ?Sized> Punctuator<'inp, L, Lang>
          for $name<S, C, MarkerLang>
        where
          L: Lexer<'inp>,
          <L::Token as Token<'inp>>::Kind: From<$name<(), (), ()>>,
        {
          #[inline(always)]
          fn name() -> CowStr {
            CowStr::from_static(stringify!([< $kind:upper >]))
          }

          #[inline(always)]
          fn description() -> Option<CowStr> {
            Some(CowStr::from_static(concat!("The `", $punct_char, "` punctuator.")))
          }

          #[inline]
          fn kind() -> <L::Token as Token<'inp>>::Kind {
            <<L::Token as Token<'inp>>::Kind as From<_>>::from(<$name>::unit())
          }
        }
      )*
    }
  };
}

define_parsers!(
  OpenAngle::open_angle::"<",
  CloseAngle::close_angle::">",
  OpenBrace::open_brace::"{",
  CloseBrace::close_brace::"}",
  OpenParen::open_paren::"(",
  CloseParen::close_paren::")",
  OpenBracket::open_bracket::"[",
  CloseBracket::close_bracket::"]",
  Comma::comma::",",
  Semicolon::semicolon::";",
  Colon::colon::":",
  Dot::dot::".",
  Tilde::tilde::"~",
  Underscore::underscore::"_",
  Equal::equal::"=",
  Hyphen::hyphen::"-",
  Arrow::arrow::"->",
  FatArrow::fat_arrow::"=>",
  PipeArrow::pipe_arrow::"|>",
  DoubleColon::double_colon::"::",
  Spread::spread::"...",
  Space::space::" ",
  Tab::tab::"\t",
  Newline::newline::"\n",
  CarriageReturn::carriage_return::"\r",
  CarriageReturnNewline::crlf::"\r\n",
  Pipe::pipe::"|",
  Ampersand::ampersand::"&",
  Percent::percent::"%",
  Slash::slash::"/",
  Backslash::backslash::"\\",
  Dollar::dollar::"$",
  Hash::hash::"#",
  At::at::"@",
  Asterisk::asterisk::"*",
  Apostrophe::apostrophe::"'",
  DoubleQuote::double_quote::"\"",
  Plus::plus::"+",
  Exclamation::exclamation::"!",
  Question::question::"?",
  Backtick::backtick::"`",
  Caret::caret::"^",

  // Equality and comparison operators
  ColonEqual::colon_equal::":=",
  LogicalEqual::logical_equal::"==",
  LogicalNotEqual::logical_not_equal::"!=",
  StrictEqual::strict_equal::"===",
  StrictNotEqual::strict_not_equal::"!==",
  LessThanOrEqual::less_than_or_equal::"<=",
  GreaterThanOrEqual::greater_than_or_equal::">=",
  StrictLessThanOrEqual::strict_less_than_or_equal::"<==",
  StrictGreaterThanOrEqual::strict_greater_than_or_equal::">==",

  // Compound assignment operators
  PlusEqual::plus_equal::"+=",
  HyphenEqual::hyphen_equal::"-=",
  AsteriskEqual::asterisk_equal::"*=",
  ExponentiationEqual::exponentiation_equal::"**=",
  SlashEqual::slash_equal::"/=",
  BackslashEqual::backslash_equal::"\\=",
  PercentEqual::percent_equal::"%=",
  AmpersandEqual::ampersand_equal::"&=",
  PipeEqual::pipe_equal::"|=",
  CaretEqual::caret_equal::"^=",
  ShlEqual::shl_equal::"<<=",
  ShrEqual::shr_equal::">>=",
  SarEqual::sar_equal::">>>=",

  // Shift operators
  ShiftLeft::shl::"<<",
  ShiftRight::shr::">>",
  ShiftArithmeticRight::sar::">>>",

  // Increment, decrement, and exponentiation
  Increment::increment::"++",
  Decrement::decrement::"--",
  Exponentiation::exponentiation::"**",

  // Logical operators
  LogicalAnd::logical_and::"&&",
  LogicalOr::logical_or::"||",

  // Null-coalescing and optional chaining
  NullCoalesce::null_coalesce::"??",
  OptionalChain::optional_chain::"?.",
);

#[cfg(all(test, feature = "std", feature = "logos"))]
mod tests;
