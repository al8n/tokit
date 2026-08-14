# 11. Anatomy of a real Tokora parser

Prerequisites: chapters 1–10.

The Calc chapters isolate one idea at a time. A maintained parser program has to make those
ideas cooperate: it owns a result model, a token model, a lexer, an error conversion boundary,
an entry point, and assertions that exercise the program as users run it. This chapter gives
that assembly order, then points to the complete programs that remain the canonical sources.

## Start with the output

Decide what a successful parse returns before choosing combinators. A calculator can fold to an
`f64`; an S-expression parser needs an AST that an evaluator consumes later; JSON borrows scalar
text while allocating collection nodes; a C expression parser builds an AST. That decision
determines whether a parser is manual recursive descent, token-level Pratt, AST-level Pratt, or
a combinator composition.

Keep parser functions generic over `Ctx`. The grammar only requires the capabilities it uses,
while the caller chooses a fail-fast or collecting emitter and the cache policy. The small
binding parser below shows the complete plumbing without becoming a second full example.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { Self } }
# #[derive(Debug, Clone, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Tok {
#   #[token("let")] Let,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[token("=")] Assign,
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))] Int(i64),
#   #[token(";")] Semi,
# }
# #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
# enum Kind { Let, Ident, Assign, Int, Semi }
# impl core::fmt::Display for Kind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Let => "let", Self::Ident => "identifier", Self::Assign => "=", Self::Int => "integer", Self::Semi => ";" })
#   }
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     core::fmt::Display::fmt(&self.kind(), f)
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = Kind;
#   type Error = LexError;
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
#   fn kind(&self) -> Kind {
#     match self { Self::Let => Kind::Let, Self::Ident => Kind::Ident, Self::Assign => Kind::Assign, Self::Int(_) => Kind::Int, Self::Semi => Kind::Semi }
#   }
#   fn is_trivia(&self) -> bool { false }
# }
# type BindingLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# #[derive(Debug, PartialEq)]
# enum ParseError { Lex, Unexpected, End }
# impl From<LexError> for ParseError { fn from(_: LexError) -> Self { Self::Lex } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>> for ParseError { fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self { Self::End } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for ParseError { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Self::End } }
# impl<'inp> From<tokora::error::token::UnexpectedTokenOf<'inp, BindingLexer<'inp>>> for ParseError {
#   fn from(_: tokora::error::token::UnexpectedTokenOf<'inp, BindingLexer<'inp>>) -> Self { Self::Unexpected }
# }
use tokora::{Emitter, InputRef, Parse, ParseContext, Parser};

fn parse_binding<'inp, Ctx>(
  input: &mut InputRef<'inp, '_, BindingLexer<'inp>, Ctx>,
) -> Result<(&'inp str, i64), ParseError>
where
  Ctx: ParseContext<'inp, BindingLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BindingLexer<'inp>, Error = ParseError>,
{
  if input.try_expect(|token| matches!(token.data(), Tok::Let))?.is_none() {
    return Err(ParseError::Unexpected);
  }
  if input.try_expect(|token| matches!(token.data(), Tok::Ident))?.is_none() {
    return Err(ParseError::Unexpected);
  }
  let name = input.slice();
  if input.try_expect(|token| matches!(token.data(), Tok::Assign))?.is_none() {
    return Err(ParseError::Unexpected);
  }
  let value = match input.next()? {
    Some(token) => match token.into_data() {
      Tok::Int(value) => value,
      _ => return Err(ParseError::Unexpected),
    },
    None => return Err(ParseError::End),
  };
  if input.try_expect(|token| matches!(token.data(), Tok::Semi))?.is_none() {
    return Err(ParseError::Unexpected);
  }
  Ok((name, value))
}

assert_eq!(
  Parser::new().apply(parse_binding).parse_str("let answer = 42;"),
  Ok(("answer", 42)),
);
```

The public surface used here is `Token`, `Lexer`, `lexer::LogosLexer`, `InputRef`,
[`InputRef::next`](crate::InputRef::next), [`InputRef::try_expect`](crate::InputRef::try_expect),
[`InputRef::slice`](crate::InputRef::slice), `ParseContext`, `Emitter`, `ParseInput`,
`TryParseInput`, [`Parser::new`](crate::Parser::new), [`Parser::apply`](crate::Parser::apply),
and [`Parse::parse_str`](crate::Parse::parse_str). Values returned from the input are spanned,
so a real parser can retain source locations as well as data.

## Build the lexical layer

Give the token enum payloads only where parsing needs values. Pair it with a payload-free kind
enum for dispatch and diagnostics, and convert lexer and structured parser errors into one
application error type. A Logos lexer is a good default; a custom `Lexer` is the variation point
when a language needs stateful or non-Logos scanning.

## Choose a parser shape

Use manual recursive descent when the next token directly chooses a grammar case. Use
combinators for regular sequencing, repetition, separators, and delimiters. Use token-level
Pratt when folds can return a token-shaped value; use AST-level Pratt when folds construct a
separate tree. Each shape can call the others—there is no all-or-nothing parser style.

## Wire the entry point

The executable boundary is deliberately boring: `Parser::new().apply(entry).parse_str(source)`.
Put the user-visible conversion from `Result` to reporting or evaluation there, not inside
low-level parser functions. This keeps the grammar reusable in tests, a CLI, and a language
server.

## Test the complete program

The four programs exercise public behavior from `main` and are also compiled as examples:

```sh
cargo run -p tokora --example calculator --features logos
cargo test -p tokora --no-default-features --features std,logos --examples
```

Small doctests verify local API contracts; the maintained binaries verify their complete
integration, including their entry points and assertion tables.

## Map the maintained examples

| Parser shape | Canonical program | Principal symbols |
| --- | --- | --- |
| Token-level Pratt evaluator | [`calculator.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/calculator.rs) | `PrattToken`, `calc_expr` |
| Manual recursive descent plus evaluation | [`s_expression.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/s_expression.rs) | `parse_expr`, `parse_list`, `eval` |
| Combinators, delimiters, and tentative choice | [`json.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/json.rs) | `try_json_value`, `json_value`, `list`, `object` |
| AST-level Pratt parser | [`c_expression.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/c_expression.rs) | `parse_lhs`, `parse_rhs`, `fold_postfix`, `parse_cexpr` |

With an output model, lexical layer, parser shape, entry point, and assertions chosen, you can
start a real parser and know which maintained program to follow. Next: the
[custom-lexer recipe](super::recipe_custom_lexer), then the walkthroughs, starting with
[chapter 12](super::ch12_calculator_example).
