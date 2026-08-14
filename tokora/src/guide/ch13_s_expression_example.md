# 13. Walkthrough: S-expressions

Prerequisites: chapters 2 and 11, plus familiarity with `Box` and `Vec`.

The maintained [`s_expression.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/s_expression.rs)
uses manual recursive descent. It deliberately avoids Pratt parsing and combinators: each branch
consumes exactly the tokens that its grammar form owns, and evaluation happens after parsing.

| Maintained program | Symbols to follow |
| --- | --- |
| [`s_expression.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/s_expression.rs) | `parse_expr`, `parse_list`, `eval`, `apply`, `Expr`, `Atom`, `BuiltIn` |

## Define tokens and the AST/value types

The lexer owns keyword strings and produces integers, booleans, built-ins, parentheses, and a
quote token. The output model distinguishes syntax (`Expr`) from evaluated values (`Atom`), while
`BuiltIn` makes functions first-class values. The public parser APIs are `Token`,
[`lexer::LogosLexer`](crate::lexer::LogosLexer), [`InputRef::next`](crate::InputRef::next),
[`InputRef::try_expect`](crate::InputRef::try_expect), `ParseContext`, `Emitter`, `Parser`, and
[`Parse::parse_str`](crate::Parse::parse_str).

## Implement atom and built-in branches in `parse_expr`

The parser consumes one token with `next` and immediately maps atom-like tokens into the AST.
The reduced example keeps only numbers and lists, but has the same recursive-descent shape as
the maintained program.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { Self } }
# #[derive(Clone, Debug, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Tok {
#   #[regex(r"-?[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))] Int(i64),
#   #[token("(")] Open,
#   #[token(")")] Close,
# }
# #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
# enum Kind { Int, Open, Close }
# impl core::fmt::Display for Kind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Int => "integer", Self::Open => "(", Self::Close => ")" })
#   }
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Display::fmt(&self.kind(), f) }
# }
# impl TokenT<'_> for Tok {
#   type Kind = Kind;
#   type Error = LexError;
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
#   fn kind(&self) -> Kind { match self { Self::Int(_) => Kind::Int, Self::Open => Kind::Open, Self::Close => Kind::Close } }
#   fn is_trivia(&self) -> bool { false }
# }
# type SExprLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# #[derive(Debug, PartialEq)]
# enum SExprError { Lex, Unexpected, End }
# impl From<LexError> for SExprError { fn from(_: LexError) -> Self { Self::Lex } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>> for SExprError { fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self { Self::End } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for SExprError { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Self::End } }
# impl<'inp> From<tokora::error::token::UnexpectedTokenOf<'inp, SExprLexer<'inp>>> for SExprError {
#   fn from(_: tokora::error::token::UnexpectedTokenOf<'inp, SExprLexer<'inp>>) -> Self { Self::Unexpected }
# }
use tokora::{Emitter, InputRef, Parse, ParseContext, Parser};

#[derive(Debug, PartialEq)]
enum Expr { Int(i64), List(Vec<Expr>) }

fn parse_expr<'inp, Ctx>(
  input: &mut InputRef<'inp, '_, SExprLexer<'inp>, Ctx>,
) -> Result<Expr, SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  match input.next()? {
    Some(token) => match token.into_data() {
      Tok::Int(value) => Ok(Expr::Int(value)),
      Tok::Open => Ok(Expr::List(parse_list(input)?)),
      Tok::Close => Err(SExprError::Unexpected),
    },
    None => Err(SExprError::End),
  }
}

fn parse_list<'inp, Ctx>(
  input: &mut InputRef<'inp, '_, SExprLexer<'inp>, Ctx>,
) -> Result<Vec<Expr>, SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  let mut values = Vec::new();
  while input.try_expect(|token| matches!(token.data(), Tok::Close))?.is_none() {
    values.push(parse_expr(input)?);
  }
  Ok(values)
}

let parsed = Parser::new().apply(parse_expr).parse_str("(1 (2 3))");
assert_eq!(parsed, Ok(Expr::List(vec![Expr::Int(1), Expr::List(vec![Expr::Int(2), Expr::Int(3)])])));
```

## Implement quote and parenthesized branches

In the complete program, `Quote` requires an opening parenthesis and then delegates to
`parse_list`. An opening parenthesis first probes for `if`; if it is present, the branch parses
condition, then-expression, and optional else-expression. Otherwise it parses a function
expression followed by its argument list. This is direct control flow, not speculative parser
choice.

## Implement `parse_list`, including the closing parenthesis

The loop's `try_expect` is the important detail: it consumes `)` when present and otherwise
leaves the next token for `parse_expr`. The list parser is therefore responsible for both the
empty list and the close delimiter; callers never consume a second close token.

## Implement `eval` and `apply`

Parsing builds syntax; evaluation reduces it. That boundary keeps parser errors separate from
runtime errors such as division by zero or applying a non-function.

```rust
#[derive(Clone, Debug, PartialEq)]
enum BuiltIn { Add, Not }
#[derive(Clone, Debug, PartialEq)]
enum Atom { Number(i64), Bool(bool), Function(BuiltIn) }
enum Expr {
  Constant(Atom),
  If { condition: Box<Expr>, then: Box<Expr>, otherwise: Option<Box<Expr>> },
  Application(Box<Expr>, Vec<Expr>),
}

fn apply(function: BuiltIn, arguments: Vec<Atom>) -> Result<Atom, String> {
  match function {
    BuiltIn::Add => arguments.into_iter().try_fold(0_i64, |sum, value| match value {
      Atom::Number(value) => Ok(sum + value),
      other => Err(format!("expected number, got {other:?}")),
    }).map(Atom::Number),
    BuiltIn::Not => match arguments.as_slice() {
      [Atom::Bool(value)] => Ok(Atom::Bool(!value)),
      _ => Err("not expects one boolean".into()),
    },
  }
}

fn eval(expr: Expr) -> Result<Atom, String> {
  match expr {
    Expr::Constant(atom) => Ok(atom),
    Expr::If { condition, then, otherwise } => match eval(*condition)? {
      Atom::Bool(true) => eval(*then),
      Atom::Bool(false) => otherwise.map(|expr| eval(*expr)).unwrap_or(Ok(Atom::Bool(false))),
      _ => Err("if condition must be boolean".into()),
    },
    Expr::Application(function, arguments) => match eval(*function)? {
      Atom::Function(function) => apply(function, arguments.into_iter().map(eval).collect::<Result<_, _>>()?),
      _ => Err("application target is not a function".into()),
    },
  }
}

let expr = Expr::Application(
  Box::new(Expr::Constant(Atom::Function(BuiltIn::Add))),
  vec![Expr::Constant(Atom::Number(1)), Expr::Constant(Atom::Number(2))],
);
assert_eq!(eval(expr), Ok(Atom::Number(3)));
```

## Exercise the maintained forms

Run [`s_expression.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/s_expression.rs)
to exercise literals, built-ins, conditionals, applications, and quoted lists:

```sh
cargo run -p tokora --example s_expression --features logos
```

The result is a complete recursive-descent parser/interpreter with no Pratt or combinator
machinery. Next: [chapter 14](super::ch14_json_example).
