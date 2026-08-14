# 14. Walkthrough: JSON

Prerequisites: chapters 3, 4, and 11.

The maintained [`json.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/json.rs)
combines borrowed scalar values with allocated `Vec` nodes for arrays and objects. It is not a
zero-allocation parser: string slices borrow from the input, while collection structure is owned.
The maintained `sample.json` supplies the end-to-end input.

| Maintained program | Symbols to follow |
| --- | --- |
| [`json.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/json.rs) | `try_json_value`, `json_value`, `list`, `object`, `JsonValue`, `JsonValue::decide` |

## Define borrowed tokens, `JsonError`, punctuator mappings, and `JsonValue`

`Token<'inp>::String(&'inp str)` borrows from the source. `JsonValue<'inp>` preserves that
borrow for scalars, but `List(Vec<JsonValue<'inp>>)` and
`Object(Vec<(&'inp str, JsonValue<'inp>)>)` allocate their container nodes. The lexer maps
punctuation through `PunctuatorToken` so the delimiter types can remain generic.

Strings arrive as **two** token variants, `String` and `EscapedString`, both reporting
`TokenKind::String`. The split is what spelling RFC 8259's surrogate-pairing rule as a regex — a
`\uXXXX` escape is admissible only outside `D800`–`DFFF`, or as a high half immediately followed
by a `\u` low half — buys for nothing: the two rules are disjoint (one forbids a backslash, the
other requires one), so the automaton is already deciding which applies. A consumer that needs
the decoded value therefore knows from the variant whether the slice can be used as it borrows or
has to be unescaped into an owned `String`. Validating the escape in a callback instead measured
1.65× the shipped rule over the bundled `sample.json`; the folded regex measures 1.02–1.07×.

```rust
use tokora::{
  Token as TokenT,
  punct::{Brace, Bracket, Colon, Comma},
  token::PunctuatorToken,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind { BraceOpen, BraceClose, BracketOpen, BracketClose, Colon, Comma }
impl core::fmt::Display for Kind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Self::BraceOpen => "{", Self::BraceClose => "}", Self::BracketOpen => "[",
      Self::BracketClose => "]", Self::Colon => ":", Self::Comma => ",",
    })
  }
}
#[derive(Clone, Debug)]
enum Token { BraceOpen, BraceClose, BracketOpen, BracketClose, Colon, Comma }
impl core::fmt::Display for Token {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Display::fmt(&self.kind(), f) }
}
impl TokenT<'_> for Token {
  type Kind = Kind;
  type Error = ();
  const READ_FRONTIER_CLASS: tokora::ReadFrontierClass = tokora::ReadFrontierClass::Unbounded;
  fn kind(&self) -> Kind {
    match self {
      Self::BraceOpen => Kind::BraceOpen, Self::BraceClose => Kind::BraceClose,
      Self::BracketOpen => Kind::BracketOpen, Self::BracketClose => Kind::BracketClose,
      Self::Colon => Kind::Colon, Self::Comma => Kind::Comma,
    }
  }
  fn is_trivia(&self) -> bool { false }
}
impl PunctuatorToken<'_> for Token {
  fn comma() -> Option<Kind> { Some(Kind::Comma) }
  fn colon() -> Option<Kind> { Some(Kind::Colon) }
  fn open_brace() -> Option<Kind> { Some(Kind::BraceOpen) }
  fn close_brace() -> Option<Kind> { Some(Kind::BraceClose) }
  fn open_bracket() -> Option<Kind> { Some(Kind::BracketOpen) }
  fn close_bracket() -> Option<Kind> { Some(Kind::BracketClose) }
}
impl From<Comma> for Kind { fn from(_: Comma) -> Self { Self::Comma } }
impl From<Colon> for Kind { fn from(_: Colon) -> Self { Self::Colon } }
impl From<Brace> for Kind { fn from(_: Brace) -> Self { Self::BraceOpen } }
impl From<Bracket> for Kind { fn from(_: Bracket) -> Self { Self::BracketOpen } }

assert_eq!(<Token as PunctuatorToken>::comma(), Some(Kind::Comma));
assert_eq!(<Token as PunctuatorToken>::open_bracket(), Some(Kind::BracketOpen));
```

The public surface includes [`token::PunctuatorToken`](crate::token::PunctuatorToken),
[`parser::expect`](crate::parser::expect), [`ParseInput::map`](crate::ParseInput::map),
[`ParseInput::ignored`](crate::ParseInput::ignored), [`ParseInput::then`](crate::ParseInput::then),
[`ParseInput::then_ignore`](crate::ParseInput::then_ignore),
[`ParseInput::separated_by_comma_while`](crate::ParseInput::separated_by_comma_while),
[`TryParseInput::separated_by_comma`](crate::TryParseInput::separated_by_comma),
[`Accumulator::collect`](crate::Accumulator::collect), [`punct::Colon`](crate::punct::Colon),
[`punct::Bracket`](crate::punct::Bracket), [`punct::Brace`](crate::punct::Brace),
[`ParseChoice::peek_then_choice`](crate::ParseChoice::peek_then_choice), and
[`ParseChoice::peek_then_try_choice`](crate::ParseChoice::peek_then_try_choice).

## Build `boolean`, `null`, `number`, and `string`

Each scalar uses `expect` to state the accepted token kind. `map` extracts a Boolean, number, or
borrowed string; `ignored` turns `null` into unit. These small parsers stay mandatory once the
value dispatcher has selected them, which gives invalid scalar starts a precise expected-kind
diagnostic.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { Self } }
# #[derive(Clone, Debug, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Token { #[token("true", |_| true)] Bool(bool), #[token("null")] Null }
# #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
# enum Kind { Bool, Null }
# impl core::fmt::Display for Kind { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str(match self { Self::Bool => "bool", Self::Null => "null" }) } }
# impl core::fmt::Display for Token { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Display::fmt(&self.kind(), f) } }
# impl TokenT<'_> for Token {
#   type Kind = Kind; type Error = LexError;
#   const READ_FRONTIER_CLASS: tokora::ReadFrontierClass = tokora::ReadFrontierClass::Unbounded;
#   fn kind(&self) -> Kind { match self { Self::Bool(_) => Kind::Bool, Self::Null => Kind::Null } }
#   fn is_trivia(&self) -> bool { false }
# }
# type JsonLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;
# #[derive(Debug, PartialEq)]
# enum JsonError { Lex, Unexpected, End }
# impl From<LexError> for JsonError { fn from(_: LexError) -> Self { Self::Lex } }
# impl<'inp> From<tokora::error::token::UnexpectedTokenOf<'inp, JsonLexer<'inp>>> for JsonError {
#   fn from(_: tokora::error::token::UnexpectedTokenOf<'inp, JsonLexer<'inp>>) -> Self { Self::Unexpected }
# }
# impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEnd<H, O, Lang, Set>> for JsonError {
#   fn from(_: tokora::error::UnexpectedEnd<H, O, Lang, Set>) -> Self { Self::End }
# }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for JsonError {
#   fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Self::End }
# }
use tokora::{Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, parser::expect, utils::Expected};

fn boolean<'inp, Ctx>(
  input: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<bool, JsonError>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
{
  expect(|token: &Token| if matches!(token, Token::Bool(_)) {
    Ok(())
  } else {
    Err(Expected::one(Kind::Bool))
  })
  .map(|token| match token { Token::Bool(value) => value, Token::Null => unreachable!() })
  .parse_input(input)
}

fn null<'inp, Ctx>(
  input: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<(), JsonError>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
{
  expect(|token: &Token| if matches!(token, Token::Null) {
    Ok(())
  } else {
    Err(Expected::one(Kind::Null))
  })
  .ignored()
  .parse_input(input)
}

fn true_then_null<'inp, Ctx>(
  input: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<bool, JsonError>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
{
  boolean.then_ignore(null).parse_input(input)
}

assert_eq!(Parser::new().apply(true_then_null).parse_str("true null"), Ok(true));
```

## Build arrays with tentative values, comma separation, delimiters, and collection

`list` passes `try_json_value` to `separated_by_comma`, collects accepted values into a `Vec`,
and sequences the bracket parsers around that comma-separated core. A tentative element must
decline without consuming when the closer is next; malformed separators remain errors. This
focused parser follows the same comma-separated array path and makes those failures executable.
For the ready-made single-region alternative to this bracket hand-roll, see the
[`brackets`](crate::parser::brackets) free shape (and its
[`parens`](crate::parser::parens)/[`braces`](crate::parser::braces)/[`angles`](crate::parser::angles)/[`delimited`](crate::parser::delimited)
siblings):

```rust
# use tokora::{
#   Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, Token as TokenT,
#   TryParseInput,
#   emitter::{FullContainerEmitter, SeparatedEmitter, UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter},
#   error::{
#     UnexpectedEot,
#     syntax::{FullContainer, MissingSyntaxOf},
#     token::{MissingTokenOf, SeparatedErrorOf, UnexpectedTokenOf},
#   },
#   logos::{self, Logos},
#   punct::{CloseBracket, Comma, OpenBracket},
#   token::PunctuatorToken,
#   try_parse_input::ParseAttempt,
# };
# #[derive(Clone, Debug, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+")]
# enum JsonToken {
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<u64>().map_err(|_| ()))] Number(u64),
#   #[token("[")] OpenBracket,
#   #[token("]")] CloseBracket,
#   #[token(",")] Comma,
# }
# #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
# enum JsonKind { Number, OpenBracket, CloseBracket, Comma }
# impl core::fmt::Display for JsonKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Number => "number", Self::OpenBracket => "[", Self::CloseBracket => "]", Self::Comma => "," })
#   }
# }
# impl core::fmt::Display for JsonToken {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Display::fmt(&self.kind(), f) }
# }
# impl TokenT<'_> for JsonToken {
#   type Kind = JsonKind;
#   type Error = ();
#   const READ_FRONTIER_CLASS: tokora::ReadFrontierClass = tokora::ReadFrontierClass::Unbounded;
#   fn kind(&self) -> JsonKind {
#     match self { Self::Number(_) => JsonKind::Number, Self::OpenBracket => JsonKind::OpenBracket, Self::CloseBracket => JsonKind::CloseBracket, Self::Comma => JsonKind::Comma }
#   }
#   fn is_trivia(&self) -> bool { false }
# }
# impl PunctuatorToken<'_> for JsonToken {
#   fn comma() -> Option<JsonKind> { Some(JsonKind::Comma) }
#   fn open_bracket() -> Option<JsonKind> { Some(JsonKind::OpenBracket) }
#   fn close_bracket() -> Option<JsonKind> { Some(JsonKind::CloseBracket) }
# }
# impl From<Comma> for JsonKind { fn from(_: Comma) -> Self { Self::Comma } }
# impl From<OpenBracket> for JsonKind { fn from(_: OpenBracket) -> Self { Self::OpenBracket } }
# impl From<CloseBracket> for JsonKind { fn from(_: CloseBracket) -> Self { Self::CloseBracket } }
# type JsonLexer<'a> = tokora::lexer::LogosLexer<'a, JsonToken>;
# #[derive(Debug, PartialEq)]
# enum JsonError { Lex, Unexpected, End, Missing, Separator, Full }
# impl From<()> for JsonError { fn from(_: ()) -> Self { Self::Lex } }
# impl<'inp> From<UnexpectedTokenOf<'inp, JsonLexer<'inp>>> for JsonError {
#   fn from(_: UnexpectedTokenOf<'inp, JsonLexer<'inp>>) -> Self { Self::Unexpected }
# }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for JsonError {
#   fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Self::End }
# }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for JsonError {
#   fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Self::End }
# }
# impl<'inp> From<MissingTokenOf<'inp, JsonLexer<'inp>>> for JsonError {
#   fn from(_: MissingTokenOf<'inp, JsonLexer<'inp>>) -> Self { Self::Missing }
# }
# impl<'inp> From<MissingSyntaxOf<'inp, JsonLexer<'inp>>> for JsonError {
#   fn from(_: MissingSyntaxOf<'inp, JsonLexer<'inp>>) -> Self { Self::Missing }
# }
# impl<'inp> From<SeparatedErrorOf<'inp, JsonLexer<'inp>>> for JsonError {
#   fn from(_: SeparatedErrorOf<'inp, JsonLexer<'inp>>) -> Self { Self::Separator }
# }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for JsonError {
#   fn from(_: FullContainer<S, Lang>) -> Self { Self::Full }
# }
# fn try_number<'inp, Ctx>(
#   input: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
# ) -> Result<ParseAttempt<u64>, JsonError>
# where
#   Ctx: ParseContext<'inp, JsonLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
# {
#   Ok(match input.try_expect(|token| matches!(token.data(), JsonToken::Number(_)))? {
#     Some(token) => match token.into_data() { JsonToken::Number(value) => ParseAttempt::Accept(value), _ => unreachable!() },
#     None => ParseAttempt::Decline,
#   })
# }
# fn array<'inp, Ctx>(
#   input: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
# ) -> Result<Vec<u64>, JsonError>
# where
#   Ctx: ParseContext<'inp, JsonLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError>
#     + SeparatedEmitter<'inp, JsonLexer<'inp>>
#     + FullContainerEmitter<'inp, JsonLexer<'inp>>
#     + UnexpectedLeadingSeparatorEmitter<'inp, JsonLexer<'inp>>
#     + UnexpectedTrailingSeparatorEmitter<'inp, JsonLexer<'inp>>,
# {
#   OpenBracket::parse
#     .ignore_then(try_number.separated_by_comma().collect())
#     .then_ignore(CloseBracket::parse)
#     .parse_input(input)
# }
assert_eq!(Parser::new().apply(array).parse_str("[1,2]"), Ok(vec![1, 2]));
assert!(Parser::new().apply(array).parse_str("[1,,2]").is_err());
assert!(Parser::new().apply(array).parse_str("[1,]").is_err());
```

## Build fields and objects with `separated_by_comma_while`

A field is `string.then_ignore(Colon::parse).then(json_value)`. `object` uses
`separated_by_comma_while` with `JsonValue::decide`, an external continuation decision over a
one-token [`cache::Peeked`](crate::cache::Peeked) window. The decision returns
[`parser::Action`](crate::parser::Action) to stop before a closing brace or continue for another
field.

## Implement tentative `try_json_value`

The six value branches are selected with [`ParseChoice::peek_then_try_choice`](crate::ParseChoice::peek_then_try_choice).
The chooser returns `Ok(None)` for a token that cannot begin a JSON value, which becomes a
[`try_parse_input::ParseAttempt`](crate::try_parse_input::ParseAttempt) decline. That is the
right behavior for an array element when the next token is a delimiter.

## Implement committed `json_value` with an expected-kind diagnostic

`json_value` uses [`ParseChoice::peek_then_choice`](crate::ParseChoice::peek_then_choice)
instead. Its chooser maps a valid start to a [`Branch`](crate::Branch) and constructs an
unexpected-token error with [`utils::Expected`](crate::utils::Expected) for every other token.
The difference is semantic: tentative choice says `not an element here`; committed choice says
`a JSON value is required here`.

## Parse `sample.json` and test separators

Run the maintained parser against its bundled source, then retain focused malformed-separator
tests beside any extension:

```sh
cargo run -p tokora --example json --features logos
```

You can now reproduce the maintained JSON parser while knowing exactly where borrowed values
end, collection allocation begins, and tentative choice is required. Next:
[chapter 15](super::ch15_c_expression_example).
