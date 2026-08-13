# Reference: vocabulary, macros & feature flags

Tokora ships a **vocabulary layer** on top of the raw combinators: ready-made punctuator and
delimiter types, a keyword generator, the classifier/expected machinery those plug into, two tiny
helper traits (`Require`/`Check`), and a `utils` grab-bag. This chapter catalogs that surface, the
two public macros that generate it, and the complete Cargo feature matrix.

The vocabulary types are all **span-generic AST fragments**: each is a `Name<S = …, C = (), Lang =
()>` carrying a span `S`, optional captured source `C`, and the language marker `Lang`. As with the
combinators, each parse entry point is **one** form generic over `Lang`, which it reads off the
input — `Comma::parse`, `If::try_parse` — so an unbranded grammar and a branded one write the
identical call. The [combinator reference](super::ref_combinators) explains that convention and
the one shape where an `_of` suffix survives. Since 0.3.0 every one of these entry points
is also generic over the input's completeness (a trailing `Cmpl` fn parameter, inferred from
the handle you pass), so the same `Comma::parse` drives complete and
[`Partial`](crate::Partial) inputs alike; if you spell an entry point's generics in full, append
the completeness argument (or `_`).

## How to read this reference

- **Signatures** are trimmed in `text` blocks; the ` ```rust ` blocks are compiling doctests.
- Doctests that drive a real parse reuse the hidden scaffold from the
  [combinator reference](super::ref_combinators): a byte-per-character
  [`Lexer`](crate::Lexer) (`CharLexer`) over single-character tokens, an `Error` that absorbs the
  taxonomy through `From`, and a concrete [`FatalContext`](crate::FatalContext) so the emitter
  `where`-clauses collapse. Value-only entries (macros that just declare a type,
  [`Expected`](crate::utils::Expected), `Require`/`Check`) need no lexer and stand alone.

---

## The `punctuator!` macro

[`punctuator!`](crate::punctuator) generates zero-sized (when `S = C = ()`) punctuator **marker
types**. Each entry is `(TypeName, "SYNTAX_TREE_LABEL", "lexeme")`.

```text
punctuator! { (Name, "LABEL", "raw"), … }
// generates, per entry:
pub struct Name<S = (), C = (), Lang: ?Sized = ()> { … }
impl Name<()>      { const UNIT: Self; const fn unit() -> Self; }
impl Name          { const fn raw() -> &'static str; }          // the "raw" lexeme
impl Name<S>       { const fn new(span: S) -> Self; }
impl Name<S, C>    { const fn with_content(span: S, content: C) -> Self; }
impl Name<S, C, Lang> { const fn as_str(&self) -> &'static str; const fn span(&self) -> &S; … }
// + Display / DisplayHuman / DisplayCompact / DisplayPretty / Borrow<str> / AsRef<str> / AsSpan / IntoSpan
```

The macro generates **only the type** — its `Display`s, span/content accessors, and `str`
comparisons. It does **not** attach a parser or a [`Punctuator`](crate::punct::Punctuator) impl;
those belong to the built-ins below (or you wire your own). Use `punctuator!` when your AST needs a
punctuation node the built-in set does not cover.

```rust
use tokora::punctuator;

punctuator! {
    /// A pipeline arrow.
    (LPipe, "L_PIPE", "<|"),
    (RPipe, "R_PIPE", "|>"),
}

// Zero-sized markers with a compile-time lexeme.
assert_eq!(LPipe::raw(), "<|");
assert_eq!(LPipe::unit().as_str(), "<|");
assert_eq!(core::mem::size_of::<LPipe>(), 0);
assert_eq!(format!("{}", RPipe::unit()), "|>");
```

---

## Built-in punctuators & the `Punctuator` trait

[`crate::punct`] ships ~80 ready-made punctuators through the same macro, each with a parse surface
and a [`Punctuator`](crate::punct::Punctuator) impl. A punctuator parses when the token stream's
current [`Token`](crate::Token) reports the matching kind — see
[`PunctuatorToken`](crate::token::PunctuatorToken) below for how a token opts in.

| Group | Types |
|-------|-------|
| **Brackets** | `OpenParen` `CloseParen` `OpenBrace` `CloseBrace` `OpenBracket` `CloseBracket` `OpenAngle` `CloseAngle` |
| **Separators / ASCII** | `Comma` `Semicolon` `Colon` `Dot` `At` `Hash` `Dollar` `Question` `Tilde` `Underscore` `Backtick` `Apostrophe` `DoubleQuote` `Backslash` |
| **Operators** | `Plus` `Hyphen` `Asterisk` `Slash` `Percent` `Caret` `Ampersand` `Pipe` `Equal` `Exclamation` |
| **Multi-char** | `Arrow` (`->`) `FatArrow` (`=>`) `PipeArrow` (`\|>`) `DoubleColon` (`::`) `Spread` (`...`) `Increment` `Decrement` `Exponentiation` `LogicalAnd` `LogicalOr` `NullCoalesce` `OptionalChain` |
| **Comparison / assign** | `LogicalEqual` `LogicalNotEqual` `StrictEqual` `LessThanOrEqual` `GreaterThanOrEqual` · `PlusEqual` `HyphenEqual` `AsteriskEqual` `SlashEqual` `ShlEqual` `ShrEqual` … |
| **Shift** | `ShiftLeft` (`<<`) `ShiftRight` (`>>`) `ShiftArithmeticRight` (`>>>`) |
| **Trivia** | `Space` `Tab` `Newline` `CarriageReturn` `CarriageReturnNewline` (alias `Crnl`) `Trivia` |

Each built-in exposes two parse entry points, both generic over `Lang`:

```text
Comma::parse(inp)      -> Result<Comma<L::Span, (), Lang>, Error>              // error on mismatch/EOI
Comma::try_parse(inp)  -> Result<ParseAttempt<Comma<L::Span, (), Lang>>, Error>// decline on mismatch
// `Lang` comes from `inp`; at `Lang = ()` the results are `Comma<L::Span, ()>` as before
// Punctuator trait: kind() -> Kind, eval(&Kind) -> bool, name(), unexpected_token(tok)
```

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
#   punct::{Comma, OpenBracket, CloseBracket, OpenParen, CloseParen, Semicolon},
#   span::Span as _,
#   token::PunctuatorToken,
# };
# #[derive(Debug, PartialEq)]
# struct Error;
# impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
# impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
# impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# #[derive(Debug, Clone, PartialEq)]
# enum Tok { Digit(u32), Ident(char), Comma, Semi, Plus, Star, LParen, RParen, LBracket, RBracket }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum Kind { Digit, Ident, Comma, Semi, Plus, Star, LParen, RParen, LBracket, RBracket }
# impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
# impl Token<'_> for Tok {
#   type Kind = Kind;
#   type Error = Infallible;
#   fn kind(&self) -> Kind { match self {
#     Tok::Digit(_) => Kind::Digit, Tok::Ident(_) => Kind::Ident, Tok::Comma => Kind::Comma,
#     Tok::Semi => Kind::Semi, Tok::Plus => Kind::Plus, Tok::Star => Kind::Star,
#     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen,
#     Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket } }
#   fn is_trivia(&self) -> bool { false }
# }
# impl PunctuatorToken<'_> for Tok {
#   fn comma() -> Option<Kind> { Some(Kind::Comma) }
#   fn semicolon() -> Option<Kind> { Some(Kind::Semi) }
#   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
#   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
#   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
#   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
# }
# impl From<Comma<(), (), ()>> for Kind { fn from(_: Comma<(), (), ()>) -> Self { Kind::Comma } }
# impl From<Semicolon<(), (), ()>> for Kind { fn from(_: Semicolon<(), (), ()>) -> Self { Kind::Semi } }
# impl From<OpenParen<(), (), ()>> for Kind { fn from(_: OpenParen<(), (), ()>) -> Self { Kind::LParen } }
# impl From<CloseParen<(), (), ()>> for Kind { fn from(_: CloseParen<(), (), ()>) -> Self { Kind::RParen } }
# impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
# impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
# struct CharLexer<'a> { src: &'a str, pos: usize, tok: SimpleSpan, state: () }
# impl<'a> Lexer<'a> for CharLexer<'a> {
#   type State = (); type Source = str; type Token = Tok; type Span = SimpleSpan; type Offset = usize;
#   fn new(src: &'a str) -> Self { Self { src, pos: 0, tok: SimpleSpan::new(0, 0), state: () } }
#   fn with_state(src: &'a str, _: ()) -> Self { Self::new(src) }
#   fn check(&self) -> Result<(), Infallible> { Ok(()) }
#   fn state(&self) -> &() { &self.state }
#   fn state_mut(&mut self) -> &mut () { &mut self.state }
#   fn into_state(self) -> Self::State {}
#   fn source(&self) -> &'a str { self.src }
#   fn span(&self) -> SimpleSpan { self.tok }
#   fn slice(&self) -> &'a str { &self.src[self.tok.start()..self.tok.end()] }
#   fn lex(&mut self) -> Option<Result<Tok, Infallible>> {
#     let bytes = self.src.as_bytes();
#     while self.pos < bytes.len() && bytes[self.pos] == b' ' { self.pos += 1; }
#     if self.pos >= bytes.len() { return None; }
#     let (start, c) = (self.pos, bytes[self.pos] as char);
#     self.pos += 1;
#     self.tok = SimpleSpan::new(start, self.pos);
#     Some(Ok(match c {
#       '0'..='9' => Tok::Digit(c as u32 - '0' as u32),
#       ',' => Tok::Comma, ';' => Tok::Semi, '+' => Tok::Plus, '*' => Tok::Star,
#       '(' => Tok::LParen, ')' => Tok::RParen, '[' => Tok::LBracket, ']' => Tok::RBracket,
#       c => Tok::Ident(c),
#     }))
#   }
#   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, Parser, parser::opt, punct::Paren, delimiter::Delimiter};
use tokora::try_parse_input::ParseAttempt;

// `Comma::parse` — consume a comma, else `UnexpectedToken` / `UnexpectedEot`.
fn a_comma<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Comma<SimpleSpan, ()>, Error> {
    Comma::parse(inp)
}
assert!(Parser::with_parser(a_comma).parse_str(",").is_ok());
assert!(Parser::with_parser(a_comma).parse_str("+").is_err());

// `Comma::try_parse` — the declining `TryParseInput` twin; `opt` turns it into an `Option`.
fn try_comma<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<ParseAttempt<Comma<SimpleSpan, ()>>, Error> {
    Comma::try_parse(inp)
}
fn maybe_comma<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Option<Comma<SimpleSpan, ()>>, Error> {
    opt(try_comma)(inp)
}
assert!(Parser::with_parser(maybe_comma).parse_str(",").unwrap().is_some());
assert!(Parser::with_parser(maybe_comma).parse_str("+").unwrap().is_none());

// Every built-in follows the same shape.
fn a_semi<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Semicolon<SimpleSpan, ()>, Error> {
    Semicolon::parse(inp)
}
assert!(Parser::with_parser(a_semi).parse_str(";").is_ok());

// `Paren` bundles `OpenParen`/`CloseParen` as a `Delimiter` and classifies kinds.
fn paren_is_open<'a>(k: &Kind) -> bool { <Paren as Delimiter<'a, CharLexer<'a>, ()>>::is_open(k) }
fn paren_is_close<'a>(k: &Kind) -> bool { <Paren as Delimiter<'a, CharLexer<'a>, ()>>::is_close(k) }
assert!(paren_is_open(&Kind::LParen));
assert!(paren_is_close(&Kind::RParen));
assert!(!paren_is_open(&Kind::RParen));
```

---

## Delimiters — the `Delimiter` trait

[`Delimiter`](crate::delimiter::Delimiter) pairs an opening and closing
[`Punctuator`](crate::punct::Punctuator). The four built-in pairs live in [`crate::punct`] and reuse
the open/close punctuators:

| Pair | Open / Close | Lexeme |
|------|--------------|--------|
| [`Paren`](crate::punct::Paren) | `OpenParen` / `CloseParen` | `( )` |
| [`Brace`](crate::punct::Brace) | `OpenBrace` / `CloseBrace` | `{ }` |
| [`Bracket`](crate::punct::Bracket) | `OpenBracket` / `CloseBracket` | `[ ]` |
| [`Angle`](crate::punct::Angle) | `OpenAngle` / `CloseAngle` | `< >` |

```text
trait Delimiter<'inp, L, Lang: ?Sized = ()> {
    type Open:  Punctuator<'inp, L, Lang>;
    type Close: Punctuator<'inp, L, Lang>;
    const KIND: DelimiterKind;        // machine identity — REQUIRED, no default
    fn name() -> CowStr;              // display string — NOT an identity
    fn is_open(&Kind) -> bool;    fn is_close(&Kind) -> bool;
    fn unexpected_open_token(tok) -> UnexpectedToken;   fn unexpected_close_token(tok) -> …;
}
```

The two identity members are not interchangeable, and this is the distinction to get right.
[`KIND`](crate::delimiter::Delimiter::KIND) is a
[`DelimiterKind`](crate::delimiter::DelimiterKind) — the pair's *machine* identity, and what
[`Unclosed::kind`](crate::error::Unclosed::kind) reports and an
[`UnclosedEmitter`](crate::emitter::UnclosedEmitter) conversion should discriminate on.
[`name`](crate::delimiter::Delimiter::name) is a display string with no uniqueness contract —
`"[]"` is correct for *any* bracket-shaped pair, so
[`Unclosed::name_ref`](crate::error::Unclosed::name_ref) is for rendering and never for
routing. A pair defined outside tokora declares
`const KIND: DelimiterKind = DelimiterKind::Custom("my_crate::DocComment")`, which is the only
variant it can *name*: the four built-ins are `#[non_exhaustive]`, matchable from outside as
`DelimiterKind::Paren { .. }` and siblings but not writable. There is deliberately **no
default** — a defaulted `KIND` would let a pair ship with an identity its author never chose.

One more fence to know about: the language brand *is* the context language. `Paren<(), (),
LangA>` is not a `Delimiter<'_, L, LangB>`, so a pair copy-pasted out of a sibling dialect
fails to compile instead of type-checking and then driving on the wrong dialect's marker.

`Delimiter` is a **classifier/error helper**, not a combinator: it recognizes the boundary kinds
and builds the boundary errors. To parse a delimited body, sequence the punctuators (as taught in
[chapter 3](super::ch03_combinators)) — `OpenParen::parse` then the body then `CloseParen::parse`,
or `open.ignore_then(body).then_ignore(close)` — or reach for the ready-made
[`delimited`](crate::parser::delimited)/[`parens`](crate::parser::parens)/[`braces`](crate::parser::braces)/[`brackets`](crate::parser::brackets)/[`angles`](crate::parser::angles)
shapes, the consumption side of these pairs, which materialize each pair's typed values through
[`TypedDelimiter`](crate::delimiter::TypedDelimiter). The `is_open`/`is_close` classification is
exercised over `Paren` in the built-in-punctuator doctest above. Balanced recovery
([chapter 8](super::ch08_recovery)) uses the same delimiter notion through
[`DelimClass`](crate::DelimClass).

---

## The `keyword!` macro & `KeywordToken`

[`keyword!`](crate::keyword) generates keyword types from `(TypeName, "SYNTAX_TREE_LABEL",
"spelling")`. Unlike `punctuator!`, the generated type **carries its own parsers** — it matches when
the token reports that canonical spelling through
[`KeywordToken::keyword`](crate::token::KeywordToken::keyword).

```text
keyword! { (If, "IF", "if"), … }
// generates, per entry (default span is SimpleSpan):
pub struct If<S = SimpleSpan, C = (), Lang: ?Sized = ()> { … }
impl If {
    fn parse(inp)     -> Result<If<L::Span, (), Lang>, Error>;               // error on mismatch/EOI
    fn try_parse(inp) -> Result<ParseAttempt<If<L::Span, (), Lang>>, Error>; // decline on mismatch
    // both generic over `Lang`, read off `inp`
}
impl Check<T, bool> for If   // predicate: does this token carry the "if" spelling?
// + UNIT / raw() / as_str() / Display / DisplayHuman / … (like punctuators)
```

The token type opts in by implementing [`KeywordToken`](crate::token::KeywordToken)
(`keyword(&self) -> Option<&'static str>`). Below, the byte-per-character scaffold can only lex a
one-character keyword, so the spelling is `"i"`; a real lexer reports the full word.

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
#   punct::{Comma, OpenBracket, CloseBracket, OpenParen, CloseParen, Semicolon},
#   span::Span as _,
#   token::PunctuatorToken,
# };
# #[derive(Debug, PartialEq)]
# struct Error;
# impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
# impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
# impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# #[derive(Debug, Clone, PartialEq)]
# enum Tok { Digit(u32), Ident(char), Comma, Semi, Plus, Star, LParen, RParen, LBracket, RBracket }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum Kind { Digit, Ident, Comma, Semi, Plus, Star, LParen, RParen, LBracket, RBracket }
# impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
# impl Token<'_> for Tok {
#   type Kind = Kind;
#   type Error = Infallible;
#   fn kind(&self) -> Kind { match self {
#     Tok::Digit(_) => Kind::Digit, Tok::Ident(_) => Kind::Ident, Tok::Comma => Kind::Comma,
#     Tok::Semi => Kind::Semi, Tok::Plus => Kind::Plus, Tok::Star => Kind::Star,
#     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen,
#     Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket } }
#   fn is_trivia(&self) -> bool { false }
# }
# impl PunctuatorToken<'_> for Tok {
#   fn comma() -> Option<Kind> { Some(Kind::Comma) }
#   fn semicolon() -> Option<Kind> { Some(Kind::Semi) }
#   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
#   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
#   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
#   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
# }
# impl<'a> tokora::token::KeywordToken<'a> for Tok {
#   fn keyword(&self) -> Option<&'static str> { match self { Tok::Ident('i') => Some("i"), _ => None } }
# }
# impl From<Comma<(), (), ()>> for Kind { fn from(_: Comma<(), (), ()>) -> Self { Kind::Comma } }
# impl From<Semicolon<(), (), ()>> for Kind { fn from(_: Semicolon<(), (), ()>) -> Self { Kind::Semi } }
# impl From<OpenParen<(), (), ()>> for Kind { fn from(_: OpenParen<(), (), ()>) -> Self { Kind::LParen } }
# impl From<CloseParen<(), (), ()>> for Kind { fn from(_: CloseParen<(), (), ()>) -> Self { Kind::RParen } }
# impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
# impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
# struct CharLexer<'a> { src: &'a str, pos: usize, tok: SimpleSpan, state: () }
# impl<'a> Lexer<'a> for CharLexer<'a> {
#   type State = (); type Source = str; type Token = Tok; type Span = SimpleSpan; type Offset = usize;
#   fn new(src: &'a str) -> Self { Self { src, pos: 0, tok: SimpleSpan::new(0, 0), state: () } }
#   fn with_state(src: &'a str, _: ()) -> Self { Self::new(src) }
#   fn check(&self) -> Result<(), Infallible> { Ok(()) }
#   fn state(&self) -> &() { &self.state }
#   fn state_mut(&mut self) -> &mut () { &mut self.state }
#   fn into_state(self) -> Self::State {}
#   fn source(&self) -> &'a str { self.src }
#   fn span(&self) -> SimpleSpan { self.tok }
#   fn slice(&self) -> &'a str { &self.src[self.tok.start()..self.tok.end()] }
#   fn lex(&mut self) -> Option<Result<Tok, Infallible>> {
#     let bytes = self.src.as_bytes();
#     while self.pos < bytes.len() && bytes[self.pos] == b' ' { self.pos += 1; }
#     if self.pos >= bytes.len() { return None; }
#     let (start, c) = (self.pos, bytes[self.pos] as char);
#     self.pos += 1;
#     self.tok = SimpleSpan::new(start, self.pos);
#     Some(Ok(match c {
#       '0'..='9' => Tok::Digit(c as u32 - '0' as u32),
#       ',' => Tok::Comma, ';' => Tok::Semi, '+' => Tok::Plus, '*' => Tok::Star,
#       '(' => Tok::LParen, ')' => Tok::RParen, '[' => Tok::LBracket, ']' => Tok::RBracket,
#       c => Tok::Ident(c),
#     }))
#   }
#   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, Parser, keyword, parser::opt};
use tokora::try_parse_input::ParseAttempt;

keyword! {
    /// The `i` keyword (one character, for the byte-per-char scaffold).
    (If, "IF", "i"),
}

assert_eq!(If::<SimpleSpan>::raw(), "i");

// `If::parse` — error when the next token is not the keyword.
fn parse_if<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<If<SimpleSpan, ()>, Error> {
    If::parse(inp)
}
assert!(Parser::with_parser(parse_if).parse_str("i").is_ok());
assert!(Parser::with_parser(parse_if).parse_str("x").is_err());

// `If::try_parse` — decline (no error) when it is not the keyword.
fn try_if<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<ParseAttempt<If<SimpleSpan, ()>>, Error> {
    If::try_parse(inp)
}
fn maybe_if<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Option<If<SimpleSpan, ()>>, Error> {
    opt(try_if)(inp)
}
assert!(Parser::with_parser(maybe_if).parse_str("i").unwrap().is_some());
assert!(Parser::with_parser(maybe_if).parse_str("x").unwrap().is_none());
```

---

## `Expected`, `OneOf` & the `Set` parameter

Token classifiers (the closure [`expect`](crate::parser::expect) takes) return an
[`Expected<Kind>`](crate::utils::Expected) describing what would have satisfied them; the token
errors carry it so `expected …, found …` diagnostics come for free.

| Item | Shape | Role |
|------|-------|------|
| [`Expected<'a, T>`](crate::utils::Expected) | `One(T)` \| `OneOf(OneOf<'a, T>)` | one expected value, or a set |
| [`Expected::one`](crate::utils::Expected::one) / [`one_of`](crate::utils::Expected::one_of) | `T` / `&[T]` | ergonomic constructors |
| [`OneOf<'a, T>`](crate::utils::OneOf) | slice/owned wrapper | the multi-alternative payload |
| `Set` (type param) | element type, default `&'static str` | the **optional** end-of-input expected set an [`UnexpectedEnd`](crate::error::UnexpectedEnd) may carry (`Option<Expected<'static, Set>>`) |

```rust
use tokora::utils::{Expected, OneOf};

let single = Expected::one("identifier");
assert_eq!(format!("{single}"), "expected 'identifier'");

let multiple = Expected::OneOf(OneOf::from_slice(&["ident", "number"]));
assert_eq!(format!("{multiple}"), "expected one of: 'ident', 'number'");

// A classifier's return value; `Kind` is the element type here.
let set: Expected<'_, &str> = Expected::one_of(&["if", "while", "for"]);
assert!(matches!(set, Expected::OneOf(_)));
```

---

## `Require` & `Check` — the predicate helpers

Two small traits abstract "does this value match a shape?" — used by the vocabulary and the
combinators.

| Trait | Method(s) | Purpose |
|-------|-----------|---------|
| [`Check<T, O = bool>`](crate::Check) | `check(&self, &T) -> O` | parser-side predicate; **any** `Fn(&T) -> O` is a `Check` (keywords implement it too). Distinct from [`Lexer::check`](crate::Lexer::check) / [`State::check`](crate::State::check). |
| [`Require<O>`](crate::Require) | `matched(&self) -> bool`, `require(self) -> Result<O, Self::Err>` | `try_into`-style extraction of a specific shape without consuming the stream. |

```rust
use tokora::{Check, Require};

// Any `Fn(&T) -> bool` is a `Check`.
let positive = |n: &i32| *n > 0;
assert!(positive.check(&5));
assert!(!positive.check(&0));

// `Require` extracts a shape, handing the value back on a miss.
#[derive(Debug, Clone, PartialEq)]
enum Punct { Dot, Comma }
#[derive(Debug, PartialEq)]
struct Dot;
impl Require<Dot> for Punct {
    type Err = Self;
    fn matched(&self) -> bool { matches!(self, Punct::Dot) }
    fn require(self) -> Result<Dot, Self::Err> {
        if self.matched() { Ok(Dot) } else { Err(self) }
    }
}
assert_eq!(Punct::Dot.require(), Ok(Dot));
assert_eq!(Punct::Comma.require(), Err(Punct::Comma));
```

---

## The `utils` grab-bag

[`crate::utils`] collects display machinery and small reusable types. The most user-facing:

| Item | What it is |
|------|-----------|
| [`Expected`](crate::utils::Expected) / [`OneOf`](crate::utils::OneOf) | expected-set machinery (above) |
| [`CowStr`](crate::utils::CowStr) | a clone-on-write string used for error/diagnostic messages |
| [`Lexeme`](crate::utils::Lexeme) / [`PositionedChar`](crate::utils::PositionedChar) | lexer-error building blocks: a character (or range) plus its offset → a span |
| [`Delimited`](crate::utils::Delimited) | an `Open`/`Close`/`Data`/span bundle for a parsed delimited construct |
| [`SingleCharEscape`](crate::utils::SingleCharEscape) / [`MultiCharEscape`](crate::utils::MultiCharEscape) / [`EscapedLexeme`](crate::utils::EscapedLexeme) | escape-sequence lexeme helpers |
| [`human_display`](crate::utils::human_display) | `DisplayHuman` + `HumanDisplay` — reader-facing rendering |
| [`sdl_display`](crate::utils::sdl_display) | `DisplaySDL` / `DisplayCompact` / `DisplayPretty` — schema-style rendering |
| [`syntax_tree_display`](crate::utils::syntax_tree_display) | `DisplaySyntaxTree` — S-expression-style tree rendering |
| [`IntoComponents`](crate::utils::IntoComponents) / [`IsAsciiChar`](crate::utils::IsAsciiChar) / [`CharLen`](crate::utils::CharLen) | decompose a parsed element / classify a byte-or-char / byte-length of a char |
| [`GenericArrayDeque`](crate::utils::GenericArrayDeque) + [`typenum`](crate::utils::typenum) | the const-capacity ring buffer used by the bounded caches/containers |
| `Maybe` / `MaybeRef` / `MaybeMut` / `Owned` / `Ref` | owned-or-borrowed helpers (re-exported from `mayber`) |

Every punctuator and keyword type implements the three display traits, so a vocabulary node renders
uniformly across human, SDL, and syntax-tree formats.

```rust
use tokora::utils::{Lexeme, PositionedChar};

// A positioned character becomes a byte span (UTF-8 aware).
let ascii = Lexeme::from(PositionedChar::with_position('a', 10));
assert_eq!(ascii.span().len(), 1);

let euro = Lexeme::from(PositionedChar::with_position('€', 20));
assert_eq!(euro.span().len(), 3);
```

---

## Public macros — the complete list

Only **two** macros are exported (`#[macro_export]`, reachable at the crate root):

| Macro | Generates |
|-------|-----------|
| [`punctuator!`](crate::punctuator) | punctuator marker types (`Name<S, C, Lang>` + displays/accessors) |
| [`keyword!`](crate::keyword) | keyword types **with** `parse`/`try_parse` + a `Check` impl |

The `separated_by_comma` / `fold_while` / `dispatch_on_kind` "families" from the
[combinator reference](super::ref_combinators) are generated **methods**, not macros; `paste` and
`seq-macro` are internal build-time dependencies with no exported surface.

---

## Feature matrix

`tokora`'s features fall into compilation tiers, a lexer adapter, source/container backends, and
tooling. `docs.rs` builds `all-features` with `--cfg docsrs`. (The
[combinator reference](super::ref_combinators) carries a one-line summary; this is the full table,
read from `Cargo.toml`.)

The versioned backends use a **two-name convention**: a friendly alias (`bytes`) turns on the
currently-supported versioned feature (`bytes_1`), which pulls the optional dependency
(`dep:bytes_1`). This is the same shape as `logos` → `logos_0_16`.

### Compilation tiers

| Feature | Enables | Implies (per `Cargo.toml`) | no_std posture |
|---------|---------|----------------------------|----------------|
| `std` *(default)* | the `std` library and the `default` features of every active dependency | `generic-arraydeque/default`, `thiserror/default`, `mayber/default`, and `<dep>?/default` for each active backend/logos version | requires `std` |
| `alloc` | `Vec`/`String`-backed containers and drivers without `std` — **not** a dynamic cache: every cache is compile-time bounded and `Window` is sealed at U1–U32, so lookahead past 32 is a transaction's job, not a buffer's | `generic-arraydeque/alloc`, `mayber/alloc`, `tinyvec_1?/alloc` | no_std **+ allocator** |
| *(neither)* | core-only parsing with bounded (array) caches/containers | — | no_std, **no alloc** |

### Lexer adapter

| Feature | Enables | Implies | no_std posture |
|---------|---------|---------|----------------|
| `logos` | the [`LogosLexer`](crate::lexer::LogosLexer) adapter for logos 0.16 | `logos_0_16` | as the dep allows |
| `logos_0_16` / `logos_0_15` / `logos_0_14` | version-pinned adapter | `dep:logos_0_16` / `_0_15` / `_0_14` | as the dep allows |

### Source backends ([`Slice`](crate::Slice) / [`Source`](crate::Source))

| Feature | Enables | Implies | no_std posture |
|---------|---------|---------|----------------|
| `bytes` | `&[u8]` / `Bytes` source | `bytes_1` → `dep:bytes_1` | needs atomic CAS |
| `bstr` | `BStr` byte-string source | `bstr_1` → `dep:bstr_1` | no_std **+ allocator**, CAS-free |
| `hipstr` | `HipStr` / `HipByt` source | `hipstr_0_8` → `dep:hipstr_0_8` | needs atomic CAS |
| `smol_bytes` | the smol-bytes source | `smol_bytes_0_1` → `dep:smol_bytes_0_1` (with the dep's `alloc` feature; smol-bytes ≥ 0.1.2) | needs atomic CAS |

"Needs atomic CAS" is a property of the dependency, not of tokora: `bytes`, `hipstr` and
`smol_bytes` all reach a refcounted buffer through `Arc`-shaped sharing whose `fetch_add` /
compare-and-swap has no lowering on targets without atomic CAS. They therefore fail to build
on a target like `thumbv6m-none-eabi`, whatever tokora does.

`bstr` is the CAS-free choice: it is a byte-string *view*, so it adds no shared ownership on
top of the allocator. That is pinned rather than asserted — CI compiles
`--target thumbv6m-none-eabi --no-default-features --features alloc,bstr_1` on every run. The
CAS-needing three are documented here rather than pinned as expected-failures, because an
expected-failure cell would go green for the wrong reason the day an upstream crate gained a
CAS-free fallback.

### Container backends ([`Container`](crate::container::Container))

| Feature | Enables | Implies | no_std posture |
|---------|---------|---------|----------------|
| `smallvec` | `SmallVec` accumulator | `smallvec_1` → `dep:smallvec_1`, **`alloc`** | requires `alloc` |
| `heapless` | `heapless::Vec` accumulator (fixed capacity) | `heapless_0_9` → `dep:heapless_0_9` | no_std-clean |
| `tinyvec` | `TinyVec` / `ArrayVec` accumulator | `tinyvec_1` → `dep:tinyvec_1` (gains `alloc` under `alloc`) | no_std-clean |

### CST, backtracking & tooling

| Feature | Enables | Implies | no_std posture |
|---------|---------|---------|----------------|
| `rowan` | the recording CST sink + typed lossless tree (the Rowan chapter) | `dep:rowan`, **`std`** | requires `std` |
| `unstable-raw` | the raw `InputRef::{save, restore, commit}` triple + `Checkpoint` construction | — | no_std-clean |
| `trace` | [`traced`](crate::traced) + combinator instrumentation | **`std`** | requires `std`; zero-cost when off |
| `conformance` | the `conformance` lexer test kit | **`std`** | requires `std` |
| `fuzz` | the `fuzz` operation-script harness | **`std`** | requires `std` |

### no_std posture, in brief

The base crate is `no-std` / `no-std::no-alloc` (its Cargo categories): drop `default` for core-only
parsing, add `alloc` for the allocator tier, or keep `std`. Features that **force `std`**: `rowan`,
`trace`, `conformance`, `fuzz`. Features that **force `alloc`**: `smallvec`. Every
other feature adds no tier floor of its own — a third-party backend compiles wherever that crate's
own `default-features = false` build does, which this table does not independently re-verify.
