# 2. First parsers

A tokora parser is a **plain function** over an [`InputRef`](crate::InputRef): pull a token,
decide, pull the next. Tokora does not eagerly materialize a whole token stream; `InputRef` pulls
from the lexer on demand and stages/caches tokens for explicit lookahead and backtracking. That
parse-while-lexing architecture is why every signature in this guide is generic over the input's
lifetime `'inp`.

The two primitives this chapter leans on:

- [`next`](crate::InputRef::next) — consume the next token unconditionally
  (`Ok(None)` at end of input);
- [`try_expect`](crate::InputRef::try_expect) — examine the next token from the cache or lexer and
  either **commit** it (the predicate matched, you get the token) or leave it staged
  (`Ok(None)`). This one-token peek-or-take is the workhorse of hand-written parsers.

## A typed error and the `Err` channel

Parsers return `Result<O, E>` where `E` is *your* error type. Failures reach it through two
routes: your own code returns it directly, and the crate's machinery **emits** structured
errors — [`UnexpectedTokenOf`](crate::error::token::UnexpectedTokenOf) when a token
mismatches, [`UnexpectedEot`](crate::error::UnexpectedEot) at a premature end, the token's
lexer error for unlexable bytes — through the configured
[`Emitter`](crate::Emitter). The default emitter, [`Fatal`](crate::emitter::Fatal),
converts the *first* emission into `E` via `From` and unwinds the parse: fail-fast, the
right default for a REPL. Chapter 7 swaps in a collecting emitter without touching the
parser. Your error type just needs the matching `From` impls (that is the
[`FromEmitterError`](crate::emitter::FromEmitterError) bound the entry points ask for).

## The fluent entry points

[`Parser::new`](crate::Parser::new) + [`apply`](crate::Parser::apply) wrap a parser
function with a default fail-fast context; the [`Parse`](crate::Parse) trait then offers
[`parse`](crate::Parse::parse), [`parse_str`](crate::Parse::parse_str),
[`parse_slice`](crate::Parse::parse_slice), and
[`parse_with_state`](crate::Parse::parse_with_state). Behind their respective source features,
the same trait also provides `parse_bytes`, `parse_bstr`, and `parse_hipstr`.

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
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
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
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser,
  error::{UnexpectedEot, token::UnexpectedTokenOf},
};

// Calc's parse error. One variant per failure family; the `From` impls are
// what let the emitter's structured errors collapse into it.
#[derive(Debug, Clone, PartialEq)]
enum CalcError {
  Lex,           // bytes that are no token at all
  Unexpected,    // a wrong token in a right place
  UnexpectedEnd, // input ended mid-statement
}

impl From<LexError> for CalcError {
  fn from(_: LexError) -> Self {
    CalcError::Lex
  }
}
impl<'inp> From<UnexpectedTokenOf<'inp, CalcLexer<'inp>>> for CalcError {
  fn from(_: UnexpectedTokenOf<'inp, CalcLexer<'inp>>) -> Self {
    CalcError::Unexpected
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for CalcError {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    CalcError::UnexpectedEnd
  }
}
impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang>
  for CalcError
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    CalcError::UnexpectedEnd
  }
}

/// Parses `let <ident> = <int> ;` and returns the binding.
///
/// The signature is the crate's idiom: generic over the parse context `Ctx`,
/// pinning only the emitter's error type. Callers choose the emitter.
fn parse_let<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<(&'inp str, i64), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  if inp.try_expect(|t| matches!(t.data(), Tok::Let))?.is_none() {
    return Err(CalcError::Unexpected);
  }
  if inp.try_expect(|t| matches!(t.data(), Tok::Ident))?.is_none() {
    return Err(CalcError::Unexpected);
  }
  // Zero-copy: `slice()` is the just-consumed token's text, borrowed from the source.
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
  Ok((name, value))
}

// The fluent entry: wrap the function, hand it a source.
let binding = Parser::new().apply(parse_let).parse_str("let answer = 42 ;");
assert_eq!(binding, Ok(("answer", 42)));

// The `Err` channel carries the typed error out.
let missing_eq = Parser::new().apply(parse_let).parse_str("let answer 42 ;");
assert_eq!(missing_eq, Err(CalcError::Unexpected));
```

## The parse context

Every signature so far has ended with the same two lines, left unexplained until now:

```text
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
```

A **parse context** is the small bundle a parse needs on top of the lexer: an
[`Emitter`](crate::Emitter) — the policy that decides what becomes of each diagnostic, the
fail-fast [`Fatal`](crate::emitter::Fatal) one by default — and the lookahead **cache** that
stages the peeked tokens from this chapter's opening. [`ParseContext`](crate::ParseContext) rolls
those two (plus an optional language marker) into one type, so a parser is generic over a single
`Ctx` knob instead of separate emitter, cache, and language parameters.

Read the two clauses in that light. The first says only "`Ctx` is *some* parse context." The
second pins the one thing the body actually depends on — that the context's emitter produces
*your* error type, so each `?` yields a `CalcError`. Everything else stays open, which is exactly
the point: the identical function runs under whatever emitter the caller installs.

You have been choosing a context all along without naming it. `Parser::new()` installs the
beginner default — [`FatalContext`](crate::FatalContext), a `Fatal` emitter over the default
cache — and `apply` substitutes it for `Ctx`. So the generic form and a copy pinned to that
concrete default are one and the same parser:

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
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
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
# use tokora::error::{UnexpectedEot, token::UnexpectedTokenOf};
# #[derive(Debug, Clone, PartialEq)]
# enum CalcError { Lex, Unexpected, UnexpectedEnd }
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'inp> From<UnexpectedTokenOf<'inp, CalcLexer<'inp>>> for CalcError {
#   fn from(_: UnexpectedTokenOf<'inp, CalcLexer<'inp>>) -> Self { CalcError::Unexpected }
# }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for CalcError {
#   fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CalcError {
#   fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { CalcError::UnexpectedEnd }
# }
use tokora::{Emitter, FatalContext, InputRef, Parse, ParseContext, Parser};

// The generic idiom, one more time — one function, any context.
fn one_int<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<i64, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Tok::Int(n) => Ok(n),
      _ => Err(CalcError::Unexpected),
    },
    None => Err(CalcError::UnexpectedEnd),
  }
}

// The concrete context `Parser::new()` installs: a fail-fast `Fatal` emitter over the default
// cache. Pinning it by hand type-checks against the generic `one_int` above — which is exactly
// the substitution `apply` performs when you never name a context at all.
fn one_int_default<'inp>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, FatalContext<'inp, CalcLexer<'inp>, CalcError>>,
) -> Result<i64, CalcError> {
  one_int(inp)
}

assert_eq!(Parser::new().apply(one_int).parse_str("42"), Ok(42));
assert_eq!(Parser::new().apply(one_int_default).parse_str("42"), Ok(42));
```

[Chapter 7](super::ch07_diagnostics) is where the generic knob earns its keep: the very same
parsers, run under a *collecting* context, gather every diagnostic instead of stopping at the
first. The full catalog — every context, emitter, and error leaf — is the
[errors, emitters & context reference](super::ref_errors_emitters_context).

## `expect`: mismatches with a name

The manual `try_expect`-then-`Err` above works, but the failure says nothing about *what*
was expected. The [`expect`](crate::parser::expect) combinator consumes one token and, on a
mismatch, routes an [`UnexpectedTokenOf`](crate::error::token::UnexpectedTokenOf) through
the emitter carrying an [`Expected`](crate::utils::Expected) — the machine-readable
"expected integer, found `;`" half of a diagnostic (chapter 7 renders it). At end of input
it emits [`UnexpectedEot`](crate::error::UnexpectedEot) instead:

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
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
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
# use tokora::{
#   Emitter, InputRef, Parse, ParseContext, Parser,
#   error::{UnexpectedEot, token::UnexpectedTokenOf},
# };
# #[derive(Debug, Clone, PartialEq)]
# enum CalcError { Lex, Unexpected, UnexpectedEnd }
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'inp> From<UnexpectedTokenOf<'inp, CalcLexer<'inp>>> for CalcError {
#   fn from(_: UnexpectedTokenOf<'inp, CalcLexer<'inp>>) -> Self { CalcError::Unexpected }
# }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for CalcError {
#   fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CalcError {
#   fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { CalcError::UnexpectedEnd }
# }
use tokora::{ParseInput, parser::expect, utils::Expected};

fn parse_int<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<i64, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  // The classifier names what it wants; a mismatch becomes a structured emission.
  let tok = expect(|t: &Tok| {
    if matches!(t, Tok::Int(_)) {
      Ok(())
    } else {
      Err(Expected::one(TokKind::Int))
    }
  })
  .parse_input(inp)?;
  match tok {
    Tok::Int(n) => Ok(n),
    _ => unreachable!("the classifier admits only integers"),
  }
}

assert_eq!(Parser::new().apply(parse_int).parse_str("7"), Ok(7));
// Mismatch: `expect` emits UnexpectedToken; Fatal converts it via `From`.
assert_eq!(
  Parser::new().apply(parse_int).parse_str(";"),
  Err(CalcError::Unexpected)
);
// Premature end: UnexpectedEot instead.
assert_eq!(
  Parser::new().apply(parse_int).parse_str(""),
  Err(CalcError::UnexpectedEnd)
);
```

`parse_let` and `parse_int` compose by ordinary function calls — that is most of tokora's
composition story already. The next chapter adds the combinator layer for the repetitive
shapes. Next: [chapter 3](super::ch03_combinators).
