# 3. Composition

Chapter 2 composed parsers with ordinary function calls. That scales surprisingly far, but
three shapes recur in every grammar — *A then B*, *zero or more A*, *A separated by commas*
— and the combinator layer expresses them declaratively. tokora has two combinator families:

- [`ParseInput`](crate::ParseInput) — a parser that must produce a value or fail. Every
  `fn(&mut InputRef<…>) -> Result<O, E>` implements it for free.
- [`TryParseInput`](crate::TryParseInput) — a parser that may also **decline**: its
  [`ParseAttempt`](crate::try_parse_input::ParseAttempt) result is either
  `Accept(value)` or `Decline`, and a decline consumes **no valid tokens** — the input is
  rewound so whatever comes next can look at the same tokens. (Lexer-*error* tokens and
  already-emitted diagnostics are not rolled back; see the
  [transactional contract](crate::try_parse_input).) Declining elements are what let the
  repetition drivers stop cleanly without arbitrary lookahead.

## Sequencing

[`then`](crate::ParseInput::then) keeps both outputs as a tuple;
[`ignore_then`](crate::ParseInput::ignore_then) and
[`then_ignore`](crate::ParseInput::then_ignore) keep one side;
[`map`](crate::ParseInput::map) transforms the output, and
[`spanned`](crate::ParseInput::spanned) / [`sliced`](crate::ParseInput::sliced) /
[`located`](crate::ParseInput::located) attach where it came from. A **delimited** shape is
just sequencing with the brackets ignored — `open.ignore_then(body).then_ignore(close)` —
which is how the argument-list example below wraps its comma list in parentheses. That
hand-roll is the lesson here; the combinator reference packages it ready-made as the
[`parens`](crate::parser::parens) shape (with [`braces`](crate::parser::braces)/[`brackets`](crate::parser::brackets)/[`angles`](crate::parser::angles)
and the generic [`delimited`](crate::parser::delimited)).

## Repetition

[`repeated`](crate::TryParseInput::repeated) drives a `TryParseInput` element until it
declines, and [`collect`](crate::Accumulator::collect) accumulates the values into any
[`Container`](crate::container::Container) (a `Vec` here; arrays and bounded containers
work too). If your element is a plain `ParseInput` and you would rather supply the
stopping decision yourself, [`repeated_while`](crate::ParseInput::repeated_while) takes an
explicit peek-window condition instead —
[`while_head`](crate::while_head) and [`while_kind`](crate::while_kind) spell the common
width-1 conditions ("continue while the head satisfies this", "…while its kind is that")
without a `Peeked` window or a turbofish. For the very common "repeat until a sentinel
token, and leave it in place" shape there is a one-liner:
[`list_until(until)`](crate::ParseInput::list_until) collects into a `Vec` and stops before
the token `until` accepts, so the caller's next step still sees it.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, PartialEq, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Tok {
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
#   Int(i64),
#   #[token("let")] Let,
#   #[token("print")] Print,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[token("+")] Plus,
#   #[token("-")] Minus,
#   #[token("*")] Star,
#   #[token("/")] Slash,
#   #[token("^")] Caret,
#   #[token("=")] Assign,
#   #[token(";")] Semi,
#   #[token(",")] Comma,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum TokKind { Int, Let, Print, Ident, Plus, Minus, Star, Slash, Caret, Assign, Semi, Comma, LParen, RParen }
# impl core::fmt::Display for TokKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Self::Int => "integer", Self::Let => "`let`", Self::Print => "`print`",
#       Self::Ident => "identifier", Self::Plus => "`+`", Self::Minus => "`-`",
#       Self::Star => "`*`", Self::Slash => "`/`", Self::Caret => "`^`",
#       Self::Assign => "`=`", Self::Semi => "`;`", Self::Comma => "`,`",
#       Self::LParen => "`(`", Self::RParen => "`)`",
#     })
#   }
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     match self {
#       Tok::Int(n) => write!(f, "{n}"),
#       other => core::fmt::Display::fmt(&other.kind(), f),
#     }
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = TokKind;
#   type Error = LexError;
#   fn kind(&self) -> TokKind {
#     match self {
#       Tok::Int(_) => TokKind::Int, Tok::Let => TokKind::Let, Tok::Print => TokKind::Print,
#       Tok::Ident => TokKind::Ident, Tok::Plus => TokKind::Plus, Tok::Minus => TokKind::Minus,
#       Tok::Star => TokKind::Star, Tok::Slash => TokKind::Slash, Tok::Caret => TokKind::Caret,
#       Tok::Assign => TokKind::Assign, Tok::Semi => TokKind::Semi, Tok::Comma => TokKind::Comma,
#       Tok::LParen => TokKind::LParen, Tok::RParen => TokKind::RParen,
#     }
#   }
#   fn is_trivia(&self) -> bool { false }
# }
# type CalcLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# use tokora::error::{UnexpectedEot, syntax::FullContainer, token::UnexpectedToken};
# #[derive(Debug, Clone, PartialEq)]
# enum CalcError { Lex, Unexpected, UnexpectedEnd }
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for CalcError {
#   fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CalcError {
#   fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for CalcError {
#   fn from(_: FullContainer<S, Lang>) -> Self { CalcError::Unexpected }
# }
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, TryParseInput,
  emitter::FullContainerEmitter,
  try_parse_input::ParseAttempt,
};

/// A `let` binding as a *try*-shaped element: decline unless the next token is
/// `let`, and only then commit to the strict tail of the statement.
fn try_let<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<(&'inp str, i64)>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  // The decision point: a non-`let` token is *put back* and we decline.
  if inp.try_expect(|t| matches!(t.data(), Tok::Let))?.is_none() {
    return Ok(ParseAttempt::Decline);
  }
  // Committed from here on: failures are real errors, not declines.
  if inp.try_expect(|t| matches!(t.data(), Tok::Ident))?.is_none() {
    return Err(CalcError::Unexpected);
  }
  let name = inp.slice();
  if inp.try_expect(|t| matches!(t.data(), Tok::Assign))?.is_none() {
    return Err(CalcError::Unexpected);
  }
  let value = match inp.next()? {
    Some(tok) => match tok.into_data() {
      Tok::Int(n) => n,
      _ => return Err(CalcError::Unexpected),
    },
    None => return Err(CalcError::UnexpectedEnd),
  };
  if inp.try_expect(|t| matches!(t.data(), Tok::Semi))?.is_none() {
    return Err(CalcError::Unexpected);
  }
  Ok(ParseAttempt::Accept((name, value)))
}

/// Zero or more bindings: repeat the element until it declines, collect into a `Vec`.
fn parse_bindings<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Vec<(&'inp str, i64)>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, CalcLexer<'inp>, Error = CalcError> + FullContainerEmitter<'inp, CalcLexer<'inp>>,
{
  use tokora::{Accumulator, ParseInput as _};
  try_let.repeated().collect().parse_input(inp)
}

let bindings = Parser::new()
  .apply(parse_bindings)
  .parse_str("let a = 1 ; let b = 2 ; let c = 3 ;")
  .unwrap();
assert_eq!(bindings, [("a", 1), ("b", 2), ("c", 3)]);

// The element declines on the first non-`let` token, so the repetition stops
// cleanly — an empty input is zero bindings, not an error.
let none = Parser::new().apply(parse_bindings).parse_str("").unwrap();
assert!(none.is_empty());
```

## Separation — separators are typed punctuators

Comma-separated lists could be hand-rolled with `try_expect`, but separator handling is
where edge cases breed: leading separators, trailing separators, doubled separators,
minimum and maximum element counts. [`separated`](crate::TryParseInput::separated) — and
its ready-made spellings like
[`separated_by_comma`](crate::TryParseInput::separated_by_comma) — puts the policy in one
place. Two small impls wire your token type to the separator vocabulary in
[`punct`](crate::punct):

- [`PunctuatorToken`](crate::token::PunctuatorToken) tells the driver which of your kinds
  *is* a comma (semicolon, parenthesis, …);
- `From<Comma<(), (), ()>>` for your kind type lets the zero-sized
  [`Comma`](crate::punct::Comma) punctuator name itself in diagnostics.

The `Separated` driver's knobs — its element-count bounds and leading/trailing separator
policies — are documented on [`Separated`](crate::parser::Separated); each reports through
its own [emitter trait](crate::emitter), which is why the `where` clause below names them.
(There is also [`separated_while`](crate::ParseInput::separated_while) for elements that
cannot decline, where you provide the lookahead condition — and
[`separated1_by::<Sep, _>(peek)`](crate::ParseInput::separated1_by), the committed-first
"light" spelling of it: one-or-more elements, an optional leading separator, a trailing one
refused, collected into a `Vec`, with the whole policy already chosen.)

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, PartialEq, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Tok {
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
#   Int(i64),
#   #[token("let")] Let,
#   #[token("print")] Print,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[token("+")] Plus,
#   #[token("-")] Minus,
#   #[token("*")] Star,
#   #[token("/")] Slash,
#   #[token("^")] Caret,
#   #[token("=")] Assign,
#   #[token(";")] Semi,
#   #[token(",")] Comma,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum TokKind { Int, Let, Print, Ident, Plus, Minus, Star, Slash, Caret, Assign, Semi, Comma, LParen, RParen }
# impl core::fmt::Display for TokKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Self::Int => "integer", Self::Let => "`let`", Self::Print => "`print`",
#       Self::Ident => "identifier", Self::Plus => "`+`", Self::Minus => "`-`",
#       Self::Star => "`*`", Self::Slash => "`/`", Self::Caret => "`^`",
#       Self::Assign => "`=`", Self::Semi => "`;`", Self::Comma => "`,`",
#       Self::LParen => "`(`", Self::RParen => "`)`",
#     })
#   }
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     match self {
#       Tok::Int(n) => write!(f, "{n}"),
#       other => core::fmt::Display::fmt(&other.kind(), f),
#     }
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = TokKind;
#   type Error = LexError;
#   fn kind(&self) -> TokKind {
#     match self {
#       Tok::Int(_) => TokKind::Int, Tok::Let => TokKind::Let, Tok::Print => TokKind::Print,
#       Tok::Ident => TokKind::Ident, Tok::Plus => TokKind::Plus, Tok::Minus => TokKind::Minus,
#       Tok::Star => TokKind::Star, Tok::Slash => TokKind::Slash, Tok::Caret => TokKind::Caret,
#       Tok::Assign => TokKind::Assign, Tok::Semi => TokKind::Semi, Tok::Comma => TokKind::Comma,
#       Tok::LParen => TokKind::LParen, Tok::RParen => TokKind::RParen,
#     }
#   }
#   fn is_trivia(&self) -> bool { false }
# }
# type CalcLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# use tokora::error::{
#   UnexpectedEot,
#   syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
#   token::{MissingToken, SeparatedError, UnexpectedToken},
# };
# #[derive(Debug, Clone, PartialEq)]
# enum CalcError { Lex, Unexpected, UnexpectedEnd }
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for CalcError {
#   fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CalcError {
#   fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for CalcError {
#   fn from(_: MissingSyntax<O, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for CalcError {
#   fn from(_: MissingToken<'a, K, O, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for CalcError {
#   fn from(_: FullContainer<S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for CalcError {
#   fn from(_: TooFew<S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for CalcError {
#   fn from(_: TooMany<S, Lang>) -> Self { CalcError::Unexpected }
# }
# use tokora::{
#   Emitter, InputRef, Parse, ParseContext, Parser, TryParseInput,
#   try_parse_input::ParseAttempt,
# };
use tokora::{
  Accumulator, ParseInput,
  emitter::{
    FullContainerEmitter, SeparatedEmitter, UnexpectedLeadingSeparatorEmitter,
    UnexpectedTrailingSeparatorEmitter,
  },
  parser::expect,
  punct::Comma,
  token::PunctuatorToken,
  utils::Expected,
};

// Wire `Tok` into the punctuator vocabulary: name which kind is the comma.
impl PunctuatorToken<'_> for Tok {
  fn comma() -> Option<TokKind> {
    Some(TokKind::Comma)
  }
}
// And let the zero-sized `Comma` punctuator name itself as a kind.
impl From<Comma<(), (), ()>> for TokKind {
  fn from(_: Comma<(), (), ()>) -> Self {
    TokKind::Comma
  }
}

/// A *try*-shaped integer element for the separated driver.
fn try_int<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  Ok(match inp.try_expect(|t| matches!(t.data(), Tok::Int(_)))? {
    Some(tok) => match tok.into_data() {
      Tok::Int(n) => ParseAttempt::Accept(n),
      _ => unreachable!("the predicate admits only integers"),
    },
    None => ParseAttempt::Decline,
  })
}

/// `( int , int , … )` — a delimited, comma-separated list: sequencing for the
/// parentheses, `separated_by_comma` for the elements.
fn parse_args<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>
    + SeparatedEmitter<'inp, CalcLexer<'inp>>
    + FullContainerEmitter<'inp, CalcLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, CalcLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, CalcLexer<'inp>>,
{
  expect(|t: &Tok| {
    if matches!(t, Tok::LParen) {
      Ok(())
    } else {
      Err(Expected::one(TokKind::LParen))
    }
  })
  .ignore_then(try_int.separated_by_comma().collect())
  .then_ignore(expect(|t: &Tok| {
    if matches!(t, Tok::RParen) {
      Ok(())
    } else {
      Err(Expected::one(TokKind::RParen))
    }
  }))
  .parse_input(inp)
}

let args: Vec<i64> = Parser::new()
  .apply(parse_args)
  .parse_str("( 1 , 2 , 3 )")
  .unwrap();
assert_eq!(args, [1, 2, 3]);

// Zero elements: the element declines at `)`, the list is empty, the closer matches.
let empty: Vec<i64> = Parser::new().apply(parse_args).parse_str("( )").unwrap();
assert!(empty.is_empty());

// A doubled separator is a structured failure, not a mis-parse.
let doubled = Parser::new().apply(parse_args).parse_str("( 1 , , 2 )");
assert!(doubled.is_err());
```

Calc now has its `print 1 , 2 ;` argument shape and statement lists. What it does *not*
have yet is a way to choose **which** statement parser to run based on the next token —
that is dispatch. Next: [chapter 4](super::ch04_dispatch).
