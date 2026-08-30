# Recipe: writing a custom lexer

Every parser in this guide runs over a **token stream**, and that stream comes from a
[`Lexer`](crate::Lexer). [Chapter 1](super::ch01_tokens) took Calc's lexer as given; this recipe
turns the seam around and builds one from scratch for a small language — a mini **config** dialect
of `key = value` entries with `#` comments:

```text
# ports
http  = 8080
debug = true
```

By the end you will have: a token vocabulary, a working lexer over it, a parser driven by that
lexer, the token *capabilities* that unlock the vocabulary layer, and a clear rule for the one
lexer decision that a lossless CST depends on. The primary path is **logos-backed** — the
[`LogosLexer`](crate::lexer::LogosLexer) adapter turns a `#[derive(Logos)]` enum into a conforming
lexer, so you write scanning *rules*, not a scanner. A brief detour covers hand-writing the
[`Lexer`](crate::Lexer) trait directly for the cases logos cannot express.

We build it in two passes. Steps 1–4 grow a **syntactic** lexer that *skips* trivia — the right
choice for an AST or evaluator — starting with whitespace. Step 5 then makes it **lossless** by
surfacing whitespace *and* comments as tokens, which is the shape a lossless CST needs.

## Step 1 — the token vocabulary

A tokora token is two types linked by the [`Token`](crate::Token) trait (the split is
[chapter 1](super::ch01_tokens)'s subject): the **token** carries payloads (`Int(i64)` holds its
value), and its [`Kind`](crate::Token::Kind) is a payload-free `Copy` discriminant that dispatch
tables and "expected one of …" diagnostics name. With logos, the token enum *is* the lexer: each
`#[token]`/`#[regex]` is a scanning rule, and a top-level `skip` drops trivia at the lexer level —
here, whitespace (Step 5 adds comments).

```rust
use tokora::{
  Lexer, Token,
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

// The raw scanner. Each attribute is a rule; `skip` discards whitespace *at the lexer level*,
// so it never reaches the parser (Step 5 revisits that decision).
#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
enum Tok {
  #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
  Int(i64),
  // Two spellings, one variant: logos gives explicit tokens priority over the identifier
  // regex, so `true`/`false` win the overlap.
  #[token("true", |_| true)]
  #[token("false", |_| false)]
  Bool(bool),
  #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
  Ident,
  #[token("=")]
  Eq,
}

// The payload-free discriminant. Its `Display` is what diagnostics print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokKind {
  Int,
  Bool,
  Ident,
  Eq,
}

impl core::fmt::Display for TokKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Self::Int => "integer",
      Self::Bool => "boolean",
      Self::Ident => "identifier",
      Self::Eq => "`=`",
    })
  }
}

// The bridge. `Token` names the kind and the lexer error, and classifies trivia.
impl Token<'_> for Tok {
  type Kind = TokKind;
  type Error = LexError;
  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

  // This lexer *skips* whitespace and comments, so no surviving token is ever trivia and
  // `SURFACES_TRIVIA` keeps its default `false`. Step 5 covers when to flip both.
  fn kind(&self) -> TokKind {
    match self {
      Tok::Int(_) => TokKind::Int,
      Tok::Bool(_) => TokKind::Bool,
      Tok::Ident => TokKind::Ident,
      Tok::Eq => TokKind::Eq,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

// The whole lexer: one type alias over the logos adapter.
type ConfigLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;

// Drive it by hand once. `slice()` borrows straight from the source — no copy — and skipped
// whitespace has already vanished from the stream.
let mut lexer = ConfigLexer::new("http = 8080\n");
let mut items = Vec::new();
while let Some(result) = lexer.lex() {
  let tok = result.expect("every non-trivia byte belongs to a token");
  items.push((tok.kind(), lexer.slice()));
}
assert_eq!(
  items,
  [
    (TokKind::Ident, "http"),
    (TokKind::Eq, "="),
    (TokKind::Int, "8080"),
  ],
);
```

That is the whole lexer. [`LogosLexer::new`](crate::Lexer::new) builds it, [`lex`](crate::Lexer::lex)
pulls one token at a time (returning `None` at end of input), and [`slice`](crate::Lexer::slice)
and [`span`](crate::Lexer::span) describe the token just produced.

## Step 2 — drive a parse

A tokora parser is a plain function over an [`InputRef`](crate::InputRef): it pulls tokens with
[`next`](crate::InputRef::next) and peeks-or-takes with [`try_expect`](crate::InputRef::try_expect),
exactly as [chapter 2](super::ch02_parsers) introduces. The parser is generic over its parse
context `Ctx` (the emitter+cache bundle — see [chapter 2](super::ch02_parsers) and the
[errors & context reference](super::ref_errors_emitters_context)), pinning only the emitter's error
type. [`Parser::new`](crate::Parser) then hands it a default fail-fast
([`Fatal`](crate::FatalContext)) context and runs it against a source.

Entry points do **not** enforce end of input, so a whole-document parser loops until the stream
runs dry itself:

```rust
# use tokora::{Lexer, Token, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, PartialEq, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Tok {
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
#   Int(i64),
#   #[token("true", |_| true)]
#   #[token("false", |_| false)]
#   Bool(bool),
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[token("=")] Eq,
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum TokKind { Int, Bool, Ident, Eq }
# impl core::fmt::Display for TokKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Int => "integer", Self::Bool => "boolean", Self::Ident => "identifier", Self::Eq => "`=`" })
#   }
# }
# impl Token<'_> for Tok {
#   type Kind = TokKind;
#   type Error = LexError;
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
#   fn kind(&self) -> TokKind {
#     match self { Tok::Int(_) => TokKind::Int, Tok::Bool(_) => TokKind::Bool, Tok::Ident => TokKind::Ident, Tok::Eq => TokKind::Eq }
#   }
#   fn is_trivia(&self) -> bool { false }
# }
# type ConfigLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser,
  error::{UnexpectedEot, token::UnexpectedTokenOf},
};

// A parsed value, and the parser's error. The `From` impls are what let the crate's
// structured errors collapse into it (the `FromEmitterError` bound the entry points ask for).
#[derive(Debug, PartialEq)]
enum Value {
  Int(i64),
  Bool(bool),
}

#[derive(Debug, Clone, PartialEq)]
enum ConfigError {
  Lex,           // bytes that are no token at all
  Unexpected,    // a wrong token in a right place
  UnexpectedEnd, // input ended mid-entry
}

impl From<LexError> for ConfigError {
  fn from(_: LexError) -> Self {
    ConfigError::Lex
  }
}
impl<'inp> From<UnexpectedTokenOf<'inp, ConfigLexer<'inp>>> for ConfigError {
  fn from(_: UnexpectedTokenOf<'inp, ConfigLexer<'inp>>) -> Self {
    ConfigError::Unexpected
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for ConfigError {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    ConfigError::UnexpectedEnd
  }
}
impl<'inp, L: Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for ConfigError {
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    ConfigError::UnexpectedEnd
  }
}

/// Parses a run of `<ident> = <int|bool>` entries to end of input.
fn config<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, ConfigLexer<'inp>, Ctx>,
) -> Result<Vec<(&'inp str, Value)>, ConfigError>
where
  Ctx: ParseContext<'inp, ConfigLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, ConfigLexer<'inp>, Error = ConfigError>,
{
  let mut entries = Vec::new();
  // Peek for a key; end of input (or any non-identifier) ends the loop cleanly.
  while inp.try_expect(|t| matches!(t.data(), Tok::Ident))?.is_some() {
    let key = inp.slice(); // zero-copy: the just-consumed identifier's text
    if inp.try_expect(|t| matches!(t.data(), Tok::Eq))?.is_none() {
      return Err(ConfigError::Unexpected);
    }
    let value = match inp.next()? {
      Some(tok) => match tok.into_data() {
        Tok::Int(n) => Value::Int(n),
        Tok::Bool(b) => Value::Bool(b),
        _ => return Err(ConfigError::Unexpected),
      },
      None => return Err(ConfigError::UnexpectedEnd),
    };
    entries.push((key, value));
  }
  Ok(entries)
}

// `Parser::new()` supplies the default fail-fast context; `.parse_str` runs it.
let parsed = Parser::new()
  .apply(config)
  .parse_str("http = 8080\ndebug = true\n");
assert_eq!(
  parsed,
  Ok(vec![("http", Value::Int(8080)), ("debug", Value::Bool(true))]),
);

// The typed error carries a wrong token out through the `Err` channel.
let bad = Parser::new().apply(config).parse_str("http 8080");
assert_eq!(bad, Err(ConfigError::Unexpected));
```

## Step 3 — token capabilities

The hand-written `matches!` and `try_expect` above work, but tokora ships a **vocabulary layer** —
ready-made punctuators, a `keyword!` generator, delimiters — that parses against your token *if*
the token opts into the matching capability trait. Each capability is a subtrait of
[`Token`](crate::Token) you implement by pointing a few methods at your `Kind`s:

- [`PunctuatorToken`](crate::token::PunctuatorToken) — map ASCII punctuation to kinds
  (`equal() -> Some(TokKind::Eq)`); unlocks the ~80 built-in [`punct`](crate::punct) types and their
  `parse`/`try_parse`, plus token-level predicates like `is_equal()`.
- [`KeywordToken`](crate::token::KeywordToken) — report a token's canonical spelling; unlocks
  [`keyword!`](crate::keyword)-generated keyword parsers.
- [`IdentifierToken`](crate::token::IdentifierToken) — flag identifier tokens.
- [`LitToken`](crate::token::LitToken) and [`PrattToken`](crate::token::PrattToken) round out the
  set for literals and Pratt expressions.

You implement only what your language uses; every method defaults to "not me". The
[vocabulary reference](super::ref_vocabulary_macros_features) is the full catalog — here is the
opt-in and one parse of each kind:

```rust
# use tokora::{Lexer, Token, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, PartialEq, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Tok {
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
#   Int(i64),
#   #[token("true", |_| true)]
#   #[token("false", |_| false)]
#   Bool(bool),
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[token("=")] Eq,
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum TokKind { Int, Bool, Ident, Eq }
# impl core::fmt::Display for TokKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Int => "integer", Self::Bool => "boolean", Self::Ident => "identifier", Self::Eq => "`=`" })
#   }
# }
# impl Token<'_> for Tok {
#   type Kind = TokKind;
#   type Error = LexError;
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
#   fn kind(&self) -> TokKind {
#     match self { Tok::Int(_) => TokKind::Int, Tok::Bool(_) => TokKind::Bool, Tok::Ident => TokKind::Ident, Tok::Eq => TokKind::Eq }
#   }
#   fn is_trivia(&self) -> bool { false }
# }
# type ConfigLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# use tokora::error::{UnexpectedEot, token::UnexpectedTokenOf};
# #[derive(Debug, Clone, PartialEq)]
# enum ConfigError { Lex, Unexpected, UnexpectedEnd }
# impl From<LexError> for ConfigError { fn from(_: LexError) -> Self { ConfigError::Lex } }
# impl<'inp> From<UnexpectedTokenOf<'inp, ConfigLexer<'inp>>> for ConfigError {
#   fn from(_: UnexpectedTokenOf<'inp, ConfigLexer<'inp>>) -> Self { ConfigError::Unexpected }
# }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for ConfigError { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { ConfigError::UnexpectedEnd } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for ConfigError { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { ConfigError::UnexpectedEnd } }
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser,
  keyword,
  punct::Equal,
  token::{IdentifierToken, KeywordToken, PunctuatorToken, PunctuatorTokenExt},
};

// Opt in: teach the token which kinds are which punctuator / keyword / identifier.
impl PunctuatorToken<'_> for Tok {
  fn equal() -> Option<TokKind> {
    Some(TokKind::Eq)
  }
}
impl KeywordToken<'_> for Tok {
  fn keyword(&self) -> Option<&'static str> {
    match self {
      Tok::Bool(true) => Some("true"),
      Tok::Bool(false) => Some("false"),
      _ => None,
    }
  }
}
impl IdentifierToken<'_> for Tok {
  fn is_identifier(&self) -> bool {
    matches!(self, Tok::Ident)
  }
}

// Token-level predicates now read the classification without matching on `Kind`.
assert_eq!(<Tok as PunctuatorToken>::equal(), Some(TokKind::Eq));
assert!(Tok::Eq.is_equal());
assert_eq!(Tok::Bool(true).keyword(), Some("true"));
assert!(Tok::Ident.is_identifier());

// A `keyword!` type parses against `KeywordToken`; it matches when the token's spelling agrees.
keyword! {
    /// The `true` literal keyword.
    (True, "TRUE", "true"),
}

// The built-in `Equal` punctuator replaces the hand-written `=` check.
fn eq_sign<'inp, Ctx>(inp: &mut InputRef<'inp, '_, ConfigLexer<'inp>, Ctx>) -> Result<(), ConfigError>
where
  Ctx: ParseContext<'inp, ConfigLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, ConfigLexer<'inp>, Error = ConfigError>,
{
  Equal::parse(inp)?;
  Ok(())
}
fn a_true<'inp, Ctx>(inp: &mut InputRef<'inp, '_, ConfigLexer<'inp>, Ctx>) -> Result<(), ConfigError>
where
  Ctx: ParseContext<'inp, ConfigLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, ConfigLexer<'inp>, Error = ConfigError>,
{
  True::parse(inp)?;
  Ok(())
}

assert!(Parser::new().apply(eq_sign).parse_str("=").is_ok());
assert!(Parser::new().apply(eq_sign).parse_str("x").is_err());
assert!(Parser::new().apply(a_true).parse_str("true").is_ok());
assert!(Parser::new().apply(a_true).parse_str("false").is_err());
```

`Equal::parse` and `True::parse` needed no new error `From` impls beyond
[`UnexpectedTokenOf`](crate::error::token::UnexpectedTokenOf) and
[`UnexpectedEot`](crate::error::UnexpectedEot) — the same two the hand-written parser already
carried. See the [vocabulary reference](super::ref_vocabulary_macros_features) for the
[`punctuator!`](crate::punctuator)/[`keyword!`](crate::keyword) macros, delimiter types, and the
`Lang`-generic spellings.

## Step 4 — the hand-written path

[`Lexer`](crate::Lexer) is a plain trait. When logos does not fit — a whitespace-sensitive or
indentation-based grammar, a lexer that must thread nesting depth through
[`State`](crate::Lexer#state-faithfulness-and-cheapness), a bespoke source type — implement it
directly. The surface is small:

```text
trait Lexer<'inp> {
    type State: State;                             // resume "mode"; cloned on every checkpoint — keep it cheap
    type Source: Source<Self::Offset> + ?Sized;   // str, [u8], a custom backend
    type Token:  Token<'inp>;
    type Span:   Span<Offset = Self::Offset> + …;
    type Offset: …;                               // usize for str / [u8]
    const SURFACES_TRIVIA: bool = <Self::Token>::SURFACES_TRIVIA;   // defaults to the vocabulary's

    fn new(src: &'inp Self::Source) -> Self;
    fn with_state(src: &'inp Self::Source, state: Self::State) -> Self;  // the *resume* constructor
    fn check(&self) -> Result<(), TokenError>;    // e.g. a resource-limit probe
    fn state(&self) -> &Self::State;
    fn state_mut(&mut self) -> &mut Self::State;
    fn into_state(self) -> Self::State;
    fn source(&self) -> &'inp Self::Source;
    fn span(&self)  -> Self::Span;                 // the current token's span
    fn slice(&self) -> SliceOf<'inp, Self>;        // the current token's text (zero-copy)
    fn lex(&mut self) -> Option<Result<Self::Token, TokenError>>;  // None = end of input (sticky)
    fn read_frontier(&self) -> ReadFrontier<Self::Offset>;   // no default: how far have you safely read?
    fn bump(&mut self, n: &Self::Offset);
}
```

The [combinator reference](super::ref_combinators) carries a complete, compiling hand-written
lexer — `CharLexer`, a byte-per-character scanner — that you can copy as a starting skeleton.

What makes it *correct* is the [lexer contract](crate::Lexer#the-lexer-contract), because the input
layer rebuilds a fresh lexer and re-lexes on demand for lookahead and backtracking. In brief:
scanning is a **pure function** of source, offset, and [`State`](crate::Lexer::State) (so replay
after a rewind is identical); [`lex`](crate::Lexer::lex) exhaustion is **sticky** (`None` stays
`None`); spans are **monotone and nonempty**; [`span`](crate::Lexer::span) and
[`slice`](crate::Lexer::slice) agree; and a **composite token owns its contents** — a string
literal or block comment is one token whose span swallows every delimiter inside it, so a `{` buried
in a string never perturbs balanced recovery. `LogosLexer` upholds every clause for you; a
hand-written lexer must uphold them itself, and the `conformance` kit (chapter 10) checks a lexer
against the contract mechanically. Partial/streaming input adds one more clause
([chapter 9](super::ch09_streaming)).

## Step 5 — trivia and losslessness

The one lexer decision with a downstream consequence: does the lexer **skip** trivia (whitespace,
comments) at the lexer level, or **surface** it as real tokens?

- **Skip it** (the config lexer above, and Calc). Trivia never reaches the parser,
  [`is_trivia`](crate::Token::is_trivia) is always `false`, and
  [`SURFACES_TRIVIA`](crate::Token::SURFACES_TRIVIA) keeps its default `false`. This is the right
  choice for a purely *syntactic* parse — an AST, an evaluator, a REPL.
- **Surface it** — drop the `skip` rules, add trivia variants whose `is_trivia` returns `true`, and
  declare [`SURFACES_TRIVIA`](crate::Token::SURFACES_TRIVIA)` = true`. That constant is a **totality
  promise**: every source byte is covered by an emitted token (trivia included) or a reported lexer
  error, none silently discarded.

A **lossless CST** (see [`crate::cst`] and the lossless-CST chapter) needs the surfacing lexer: its
gap-filling `cst::Sink` tiles the whole source, and a skipped-trivia gap is
indistinguishable at the event level from a *dropped token*. So the lossless
(`gap_kind`) sink **refuses at compile time** to be built over a lexer
that does not declare `SURFACES_TRIVIA` — the guarantee is enforced, not hoped for. (Declaring
`true` while still skipping is a contract violation surfaced as
`cst::FinishError::UncoveredGap`, never UB.)

Here is the config vocabulary rebuilt to surface trivia — the shape a lossless parse requires:

```rust
use tokora::{Lexer, Token, span::Span as _, logos::{self, Logos}};

#[derive(Clone, Debug, Default, PartialEq)]
struct LexError;
impl From<()> for LexError {
  fn from(_: ()) -> Self {
    LexError
  }
}

// A *lossless* vocabulary: whitespace and comments are real tokens — note the absence of `skip`.
#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, error = LexError)]
enum Lossless {
  #[regex(r"[ \t\r\n]+")]
  Whitespace,
  #[regex(r"#[^\r\n]*", allow_greedy = true)]
  Comment,
  #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
  Int(i64),
  #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
  Ident,
  #[token("=")]
  Eq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum LosslessKind {
  Whitespace,
  Comment,
  Int,
  Ident,
  Eq,
}

impl core::fmt::Display for LosslessKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Self::Whitespace => "whitespace",
      Self::Comment => "comment",
      Self::Int => "integer",
      Self::Ident => "identifier",
      Self::Eq => "`=`",
    })
  }
}

impl Token<'_> for Lossless {
  type Kind = LosslessKind;
  type Error = LexError;
  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

  // The totality promise the lossless CST sink checks at compile time.
  const SURFACES_TRIVIA: bool = true;

  fn kind(&self) -> LosslessKind {
    match self {
      Lossless::Whitespace => LosslessKind::Whitespace,
      Lossless::Comment => LosslessKind::Comment,
      Lossless::Int(_) => LosslessKind::Int,
      Lossless::Ident => LosslessKind::Ident,
      Lossless::Eq => LosslessKind::Eq,
    }
  }

  // The per-token identity half: which surfaced tokens are trivia.
  fn is_trivia(&self) -> bool {
    matches!(self, Lossless::Whitespace | Lossless::Comment)
  }
}

// The `LogosLexer` adapter is one blanket impl for every token type, so a logos-backed dialect
// declares `SURFACES_TRIVIA` on its `Token` impl (above); `Lexer::SURFACES_TRIVIA` inherits it.
// A hand-written lexer whose skipping differs from its vocabulary overrides the `Lexer` const.
type LosslessLexer<'a> = tokora::lexer::LogosLexer<'a, Lossless>;

// Every byte is now a token — trivia included — so the spans tile the source with no gaps.
let mut lexer = LosslessLexer::new("x = 1 # note");
let mut spans = Vec::new();
while let Some(result) = lexer.lex() {
  result.expect("every byte belongs to a token");
  spans.push((lexer.span().start(), lexer.span().end()));
}
assert_eq!(spans.len(), 7); // ident, ws, `=`, ws, int, ws, comment
assert_eq!(spans.first().map(|s| s.0), Some(0)); // cover starts at the first byte …
assert_eq!(spans.last().map(|s| s.1), Some(12)); // … and reaches the last, contiguously
```

That is the entire difference between a syntactic lexer and a lossless one: no `skip`, trivia
variants that answer `is_trivia`, and the `SURFACES_TRIVIA` promise. Pick the first for an AST or
evaluator; pick the second when you need to reconstruct the source exactly — the lossless-CST
chapter builds a full typed tree on top of it.

Next: the walkthroughs ([calculator](super::ch12_calculator_example),
[JSON](super::ch14_json_example)) each define a lexer with these steps, then build a real grammar
over it.
