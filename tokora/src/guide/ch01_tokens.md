# 1. Tokens and the lexer

Calc's source text becomes a stream of **tokens** before any parsing happens. This chapter
defines that token type, wires it to a lexer, and states the contract the rest of the guide
(and the crate) relies on.

## The token type — data and kind

A tokora token is two types working together, connected by the [`Token`](crate::Token) trait:

- the **token** itself (`Tok` below) carries payloads — `Int(i64)` holds its value;
- its [`Kind`](crate::Token::Kind) (`TokKind` below) is a payload-free, `Copy` discriminant.

The split matters later: dispatch tables (chapter 4) and "expected one of …" diagnostics
(chapter 7) need to *name* token classes without inventing payload values, and that is
exactly what a kind is. [`Token::is_trivia`](crate::Token::is_trivia) marks tokens (like
whitespace or comments) that carry no syntax; Calc has none because the lexer skips
whitespace outright — languages that keep trivia tokens instead skip them with
[`padded`](crate::ParseInput::padded) at the parser level.

## The lexer

Any type implementing [`Lexer`](crate::Lexer) can drive tokora's parsers. Calc does not
hand-roll one: the [`LogosLexer`](crate::lexer::LogosLexer) adapter turns any
[`logos`](crate::logos)-derived token enum into a conforming lexer, so the whole lexer is
the `#[derive(Logos)]` block below. Keywords are plain `#[token]` patterns — logos resolves
the `let`-versus-identifier overlap by longest match, then pattern priority.

```rust
use tokora::{
  Lexer, SimpleSpan, Token as TokenT,
  logos::{self, Logos},
};

// The lexer-level error: what lexing yields for bytes that are no token at all.
#[derive(Clone, Debug, Default, PartialEq)]
struct LexError;

impl From<()> for LexError {
  fn from(_: ()) -> Self {
    LexError
  }
}

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
enum Tok {
  #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
  Int(i64),
  #[token("let")]
  Let,
  #[token("print")]
  Print,
  #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
  Ident,
  #[token("+")]
  Plus,
  #[token("-")]
  Minus,
  #[token("*")]
  Star,
  #[token("/")]
  Slash,
  #[token("^")]
  Caret,
  #[token("=")]
  Assign,
  #[token(";")]
  Semi,
  #[token(",")]
  Comma,
  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
}

// The payload-free discriminant. `Display` is what diagnostics print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokKind {
  Int,
  Let,
  Print,
  Ident,
  Plus,
  Minus,
  Star,
  Slash,
  Caret,
  Assign,
  Semi,
  Comma,
  LParen,
  RParen,
}

impl core::fmt::Display for TokKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Self::Int => "integer",
      Self::Let => "`let`",
      Self::Print => "`print`",
      Self::Ident => "identifier",
      Self::Plus => "`+`",
      Self::Minus => "`-`",
      Self::Star => "`*`",
      Self::Slash => "`/`",
      Self::Caret => "`^`",
      Self::Assign => "`=`",
      Self::Semi => "`;`",
      Self::Comma => "`,`",
      Self::LParen => "`(`",
      Self::RParen => "`)`",
    })
  }
}

impl core::fmt::Display for Tok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Tok::Int(n) => write!(f, "{n}"),
      other => core::fmt::Display::fmt(&other.kind(), f),
    }
  }
}

// The bridge: tokora's `Token` trait names the kind and the lexer error type.
impl TokenT<'_> for Tok {
  type Kind = TokKind;
  type Error = LexError;
  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

  fn kind(&self) -> TokKind {
    match self {
      Tok::Int(_) => TokKind::Int,
      Tok::Let => TokKind::Let,
      Tok::Print => TokKind::Print,
      Tok::Ident => TokKind::Ident,
      Tok::Plus => TokKind::Plus,
      Tok::Minus => TokKind::Minus,
      Tok::Star => TokKind::Star,
      Tok::Slash => TokKind::Slash,
      Tok::Caret => TokKind::Caret,
      Tok::Assign => TokKind::Assign,
      Tok::Semi => TokKind::Semi,
      Tok::Comma => TokKind::Comma,
      Tok::LParen => TokKind::LParen,
      Tok::RParen => TokKind::RParen,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

// The whole lexer: one type alias over the logos adapter.
type CalcLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;

// Drive it by hand once — parsers will do this for us from chapter 2 on.
let mut lexer = CalcLexer::new("let answer = 6 * 7 ;");
let mut tokens = Vec::new();
while let Some(result) = lexer.lex() {
  let tok = result.expect("every byte of this source belongs to a token");
  // `span()` and `slice()` describe the token just lexed — the slice borrows
  // straight from the source, no copy.
  tokens.push((tok.kind(), lexer.slice(), lexer.span()));
}

assert_eq!(tokens[0], (TokKind::Let, "let", SimpleSpan::new(0, 3)));
assert_eq!(tokens[1], (TokKind::Ident, "answer", SimpleSpan::new(4, 10)));
assert_eq!(tokens[2], (TokKind::Assign, "=", SimpleSpan::new(11, 12)));
assert_eq!(
  tokens[3..]
    .iter()
    .map(|(k, _, _)| *k)
    .collect::<Vec<_>>(),
  [TokKind::Int, TokKind::Star, TokKind::Int, TokKind::Semi],
);
```

## The lexer contract, in brief

Parsers do more than drain the lexer forward: they peek, checkpoint, rewind, and re-lex.
That machinery is only sound if the lexer behaves like a **pure function of source,
position, and state** — the same position always yields the same token, spans never move
backward, exhaustion is sticky, and a composite token (a string literal, say) owns every
byte it spans. The full, normative statement lives in
[the `Lexer` contract](crate::Lexer#the-lexer-contract); `LogosLexer` upholds it for you,
and chapter 10 shows the `conformance` kit that checks a hand-rolled
lexer against it mechanically.

One consequence worth internalizing now: because the lexer is deterministic and the source
is immutable, **rewinding is cheap** — a checkpoint is a snapshot, not a journal. That is
what makes the backtracking of chapter 6 and the recovery of chapter 8 affordable.

Next: [chapter 2](super::ch02_parsers) writes the first parsers over this token stream.
