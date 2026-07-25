# Reference: combinators & atoms

The tutorial chapters build Calc with a curated slice of the combinator surface. This chapter is
the **catalog**: every combinator method and free atom, the `many/` driver builder, the error
taxonomy, and the feature matrix — each entry with its real signature and a tiny compiling use.

The two core traits are [`ParseInput`](crate::ParseInput) (must produce a value or fail) and
[`TryParseInput`](crate::TryParseInput) (may also *decline* without consuming a valid token); both
are introduced in [chapter 2](super::ch02_parsers) and [chapter 3](super::ch03_combinators). A
plain `fn(&mut InputRef<…>) -> Result<O, E>` is a `ParseInput`, and a
`fn(&mut InputRef<…>) -> Result<ParseAttempt<O>, E>` is a `TryParseInput`, so most atoms are just
constructors for these shapes.

Since 0.3.0 both traits carry a defaulted completeness parameter
(`Cmpl = `[`Complete`](crate::Complete)), mirroring [`InputRef`](crate::InputRef), so every
signature in this reference reads unchanged; the parameter exists for when a parser must run
under [`Partial`](crate::Partial) input ([chapter 9](super::ch09_streaming)). **The mode
legend for this catalog:** the try/consume-channel families are mode-generic — the leaf atoms
(`expect`/`any`/`Ident`/keywords/puncts), every pass-through adapter (`map*`, `filter*`,
`validate*`, `then*`, `ignored`, `padded*`, `recover`/`skip_then_retry`, `spanned`/`sliced`/
`located`), the try-driven collections (`repeated`, `separated*`, `fold`/`rfold`, `collect`),
and the delimited shapes. Two families stay **Complete-only** this release, each pinned with
its reason recorded on the impl: the decision-window class (`*_while`, `peek_*`,
`dispatch_*`, pratt — a non-final frontier can silently truncate their peeked decision
window, which would read as "construct ended") and the CST `node` family (partial event
semantics is a separately-deferred design). Reaching for a pinned combinator from partial
code fails at the drive site — an `E0277`-family "not implemented for … `Partial`" wall (in
method-call position it surfaces as `E0599`) — never a silent wrong parse.

## How to read this reference

- **Signatures** are shown trimmed (the always-present `Self: Sized`, `L: Lexer`,
  `Ctx: ParseContext` bounds are elided) in `text` blocks; the compiling ` ```rust ` blocks below
  each family show minimal uses.
- Every example shares one hidden scaffold: a minimal hand-written
  [`Lexer`](crate::Lexer) — `CharLexer` — over single-character tokens (`Digit`, `Ident`, and the
  punctuation `,` `;` `+` `*` `(` `)` `[` `]`), plus an `Error` that absorbs the whole taxonomy
  through `From`. [Chapter 1](super::ch01_tokens) shows the real logos-based lexer.
- The examples fix a **concrete** context — [`FatalContext`](crate::FatalContext), whose
  [`Fatal`](crate::emitter::Fatal) emitter implements every capability trait — so the emitter
  `where`-clauses you see in the tutorials collapse to nothing here. To write a parser reusable
  across emitters, keep it generic over `Ctx: ParseContext` and name the capabilities it needs
  (see [chapter 3](super::ch03_combinators)); the reference stays concrete for brevity.

## The `_of` / `Lang` convention

Every trait, type, and function carries a language marker `Lang: ?Sized = ()`. Most atoms come in
two spellings: a **base** form that fixes `Lang = ()`, and an **`_of`** form generic over `Lang`
— [`expect`](crate::parser::expect)/[`expect_of`](crate::parser::expect_of),
[`fail`](crate::parser::fail)/`fail_of`, [`Any::new`](crate::parser::Any::new)/`Any::of`,
[`Parser::new`](crate::Parser::new)/`Parser::of`, `Comma::try_parse`/`try_parse_of`. The pair is
otherwise identical; reach for the base form unless you are building a language-generic library.
Repetition-count knobs (`at_least`, …) and the `separated_by_*` family have no `_of` twin — they
inherit `Lang` from the parser they wrap.

---

## Atoms — parsers from nothing

| Atom | Produces | One-liner |
|------|----------|-----------|
| [`Any::of()`](crate::parser::Any) | `L::Token` | consume one token of any kind; errors only at end of input |
| [`expect(check)`](crate::parser::expect) | `L::Token` | consume one token satisfying `check`, else a typed `UnexpectedToken` |
| [`try_expect(check)`](crate::parser::try_expect) | `L::Token` | the [`TryParseInput`](crate::TryParseInput) twin: *decline* instead of error |
| [`Empty::new()`](crate::parser::Empty) | `()` | always succeed, consume nothing (the sequencing identity) |
| [`Todo::new()`](crate::parser::Todo) | `O` | type-checks as any parser, **panics** if run — a placeholder |
| [`fail(f)`](crate::parser::fail) | `O` | always fail with `f()` |

```text
Any::of() -> Any<L, Ctx, Lang>                       // also ::spanned_of/::sliced_of/::located_of
expect(check) -> Expect<Classifier, Ctx>             // check: FnMut(&Token) -> Result<(), Expected<Kind>>
Empty::new() -> Empty                                 // Todo::<O>::new() -> Todo<O>
fail(f) -> Fail<F, L, O, Ctx>                         // f: FnMut() -> Error
```

[`Any`](crate::parser::Any) also has `::spanned()`, `::sliced()`, and `::located()` constructors
that attach position/text to the token. [`expect`](crate::parser::expect) is preferred over
`Any` + [`filter`](crate::ParseInput::filter) because it produces an
[`expected …, found …`](crate::error::token::UnexpectedToken) diagnostic from the
[`Expected`](crate::utils::Expected) the classifier returns.

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, Parser, ParseInput as _, parser::{Any, Empty, expect, fail}, utils::Expected};

// `Any::of()` — any one token.
fn first<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Tok, Error> {
    Any::of().parse_input(inp)
}
assert_eq!(Parser::with_parser(first).parse_str("+").unwrap(), Tok::Plus);

// `expect(check)` — a specific token, with a typed error otherwise.
fn a_plus<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Tok, Error> {
    expect(|t: &Tok| if matches!(t, Tok::Plus) { Ok(()) } else { Err(Expected::one(Kind::Plus)) })
        .parse_input(inp)
}
assert_eq!(Parser::with_parser(a_plus).parse_str("+").unwrap(), Tok::Plus);
assert!(Parser::with_parser(a_plus).parse_str("*").is_err());

// `Empty::new()` — succeed, consuming nothing; `fail(f)` — always fail.
fn nothing<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<(), Error> {
    Empty::new().parse_input(inp)
}
fn boom<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<(), Error> {
    fail(|| Error).parse_input(inp)
}
assert!(Parser::with_parser(nothing).parse_str("").is_ok());
assert!(Parser::with_parser(boom).parse_str("+").is_err());
```

---

## Transforming output

All are methods on [`ParseInput`](crate::ParseInput). Each has a `_with` twin that additionally
receives a [`ParseState`](crate::ParseState) (span/slice access) — `map_with`, `filter_with`,
`filter_map_with`, `validate_with`.

| Method | Shape | One-liner |
|--------|-------|-----------|
| [`map`](crate::ParseInput::map) | `FnMut(O) -> U` | transform the output |
| [`filter`](crate::ParseInput::filter) | `FnMut(&O) -> Result<(), E>` | keep the value, or fail |
| [`filter_map`](crate::ParseInput::filter_map) | `FnMut(O) -> Result<U, E>` | transform-or-fail in one step |
| [`validate`](crate::ParseInput::validate) | `FnMut(&O) -> Result<(), E>` | assert an invariant, keep the value |
| [`.unwrap()`](crate::ParseInputUnwrapExt::unwrap) | `Option<O>` → `O` | unwrap an `Option` output, **panic** on `None` |

```text
map<U, F>(self, f: F) -> Map<…>                       // F: FnMut(O) -> U
filter<F>(self, f: F) -> Filter<…>                    // F: FnMut(&O) -> Result<(), Error>
filter_map<U, F>(self, f: F) -> FilterMap<…>          // F: FnMut(O) -> Result<U, Error>
validate<F>(self, f: F) -> Validate<…>                // F: FnMut(&O) -> Result<(), Error>
unwrap(self) -> Unwrapped<…>                          // where Self: ParseInput<Option<O>>
```

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, Parser, ParseInput as _, ParseInputUnwrapExt as _, parser::{expect, opt}, utils::Expected};
use tokora::try_parse_input::ParseAttempt;

fn a_digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Tok, Error> {
    expect(|t: &Tok| if matches!(t, Tok::Digit(_)) { Ok(()) } else { Err(Expected::one(Kind::Digit)) })
        .parse_input(inp)
}
fn try_digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<ParseAttempt<u32>, Error> {
    Ok(match inp.try_expect(|t| matches!(t.data(), Tok::Digit(_)))? {
        Some(sp) => match sp.into_data() { Tok::Digit(n) => ParseAttempt::Accept(n), _ => unreachable!() },
        None => ParseAttempt::Decline,
    })
}

// filter_map: token → value, or fail
fn value<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    a_digit.filter_map(|t| match t { Tok::Digit(n) => Ok(n), _ => Err(Error) }).parse_input(inp)
}
assert_eq!(Parser::with_parser(value).parse_str("7").unwrap(), 7);

// map + validate: value, then assert it is even
fn even<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    a_digit
        .map(|t| match t { Tok::Digit(n) => n, _ => 0 })
        .validate(|n: &u32| if n % 2 == 0 { Ok(()) } else { Err(Error) })
        .parse_input(inp)
}
assert_eq!(Parser::with_parser(even).parse_str("4").unwrap(), 4);
assert!(Parser::with_parser(even).parse_str("5").is_err());

// unwrap: an `opt` Option output, unwrapped
fn required<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    opt(try_digit).unwrap().parse_input(inp)
}
assert_eq!(Parser::with_parser(required).parse_str("9").unwrap(), 9);
```

---

## Shaping — spans, slices, ignoring

These methods on [`ParseInput`](crate::ParseInput) re-shape the output without changing what is
consumed. `spanned`/`sliced`/`located` are taught in [chapter 3](super::ch03_combinators).

| Method | Produces |
|--------|----------|
| [`spanned`](crate::ParseInput::spanned) | `Spanned<O, Span>` — output + its source span |
| [`sliced`](crate::ParseInput::sliced) | `Sliced<O, Slice>` — output + its source text |
| [`located`](crate::ParseInput::located) | `Located<O, Span, Slice>` — output + span **and** text |
| [`ignored`](crate::ParseInput::ignored) | `()` — discard the output, keep the consumption |
| [`by_ref`](crate::ParseInput::by_ref) | `&mut ByRef<Self>` — reuse a parser without moving it |

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, Parser, ParseInput as _, parser::Any, span::Spanned};

// `.spanned()` wraps the output with the span it covers.
fn spanned<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Spanned<Tok, SimpleSpan>, Error> {
    Any::of().spanned().parse_input(inp)
}
let sp = Parser::with_parser(spanned).parse_str("+").unwrap();
assert_eq!(sp.data, Tok::Plus);
assert_eq!((sp.span.start(), sp.span.end()), (0, 1));

// `.ignored()` keeps the consumption, drops the value.
fn ignored<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<(), Error> {
    Any::of().ignored().parse_input(inp)
}
assert!(Parser::with_parser(ignored).parse_str("+").is_ok());
```

---

## Sequencing

All methods on [`ParseInput`](crate::ParseInput). A **delimited** shape is just sequencing with
the brackets ignored — `open.ignore_then(body).then_ignore(close)` — packaged ready-made by
[`delimited`](crate::parser::delimited) and
[`parens`](crate::parser::parens)/[`braces`](crate::parser::braces)/[`brackets`](crate::parser::brackets)/[`angles`](crate::parser::angles)
(see [*Delimited shapes*](#delimited-shapes)).

| Method | Keeps | One-liner |
|--------|-------|-----------|
| [`then`](crate::ParseInput::then) | `(O, U)` | parse both, keep both |
| [`then_ignore`](crate::ParseInput::then_ignore) | `O` | parse both, keep the first |
| [`ignore_then`](crate::ParseInput::ignore_then) | `U` | parse both, keep the second |
| [`then_value`](crate::ParseInput::then_value) | `U` | parse `self`, discard it, yield `f()` |
| [`and_then`](crate::ParseInput::and_then) | `U` | map the first output fallibly (`FnMut(O) -> Result<U, E>`) |

```text
then<T, U>(self, second: T) -> Then<…>               // second: ParseInput<U>
then_ignore<G, U>(self, second: G) -> ThenIgnore<…>
ignore_then<G, U>(self, second: G) -> IgnoreThen<…>
then_value<F, U>(self, value: F) -> ThenValue<…>     // value: FnMut() -> U
and_then<T, U>(self, f: T) -> AndThen<…>             // f: FnMut(O) -> Result<U, Error>
```

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, Parser, ParseInput as _};

fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    match inp.next()? { Some(sp) => match sp.into_data() { Tok::Digit(n) => Ok(n), _ => Err(Error) }, None => Err(Error) }
}
fn plus<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<(), Error> {
    match inp.next()? { Some(sp) if matches!(sp.data(), Tok::Plus) => Ok(()), _ => Err(Error) }
}

fn pair<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<(u32, u32), Error> {
    digit.then(digit).parse_input(inp)                    // both outputs
}
fn lhs<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    digit.then_ignore(plus).parse_input(inp)              // keep the digit, drop the `+`
}
fn rhs<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    plus.ignore_then(digit).parse_input(inp)              // drop the `+`, keep the digit
}
fn tagged<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<&'static str, Error> {
    plus.then_value(|| "op").parse_input(inp)             // consume `+`, yield a fixed value
}
fn nonzero<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    digit.and_then(|n| if n > 0 { Ok(n) } else { Err(Error) }).parse_input(inp)
}

assert_eq!(Parser::with_parser(pair).parse_str("12").unwrap(), (1, 2));
assert_eq!(Parser::with_parser(lhs).parse_str("1+").unwrap(), 1);
assert_eq!(Parser::with_parser(rhs).parse_str("+1").unwrap(), 1);
assert_eq!(Parser::with_parser(tagged).parse_str("+").unwrap(), "op");
assert!(Parser::with_parser(nonzero).parse_str("0").is_err());
```

---

## Optional & choice

[`opt(p)`](crate::parser::opt) adapts a declining `try_`-parser into one that yields `Option`
(`Some` on accept, `None` on decline, with nothing consumed on decline). Choice is a **tuple**
impl — a tuple of up to 32 parsers is a [`ParseChoice`](crate::ParseChoice), and you drive it with
one of the deterministic selectors (taught in [chapter 4](super::ch04_dispatch)):

| Selector | On | One-liner |
|----------|-----|-----------|
| [`dispatch_on_kind(table)`](crate::ParseChoice::dispatch_on_kind) | `(P0, …)` | pick branch `i` when the next token's kind is `table[i]`; the whole table becomes the expected set on a miss |
| [`peek_then_choice(h)`](crate::ParseChoice::peek_then_choice) | `(P0, …)` | you write the decision from a peek window, returning the branch id |
| [`peek_then_try_choice(h)`](crate::ParseChoice::peek_then_choice) | `(P0, …)` | as above, but the handler may return `None` to decline |
| [`fused_dispatch_on_kind(table)`](crate::ParseTokenChoice::fused_dispatch_on_kind) | `(F0, …)` | like `dispatch_on_kind`, but each arm is `FnMut(head, inp)` and the head token is lexed once |

[`opt`](crate::parser::opt) and [`.accepted()`](crate::try_parse_input::TryParseInput::accepted)
(which turns a `TryParseInput` into a `ParseInput<ParseAttempt<O>>`/`ParseInput<Option<O>>`) are
the bridges between the two trait worlds.

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, ParseChoice as _, ParseInput as _, Parser, parser::opt};
use tokora::try_parse_input::ParseAttempt;

fn try_digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<ParseAttempt<u32>, Error> {
    Ok(match inp.try_expect(|t| matches!(t.data(), Tok::Digit(_)))? {
        Some(sp) => match sp.into_data() { Tok::Digit(n) => ParseAttempt::Accept(n), _ => unreachable!() },
        None => ParseAttempt::Decline,
    })
}

// `opt`: Some on a digit, None (nothing consumed) otherwise.
fn maybe_digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Option<u32>, Error> {
    opt(try_digit)(inp)
}
assert_eq!(Parser::with_parser(maybe_digit).parse_str("3").unwrap(), Some(3));
assert_eq!(Parser::with_parser(maybe_digit).parse_str("+").unwrap(), None);

// `dispatch_on_kind`: a static table names each branch's first token.
fn digit_branch<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    match inp.next()? { Some(sp) => match sp.into_data() { Tok::Digit(n) => Ok(n), _ => Err(Error) }, None => Err(Error) }
}
fn plus_branch<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    inp.next()?;
    Ok(0)
}
fn choose<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    static TABLE: [Kind; 2] = [Kind::Digit, Kind::Plus];
    (digit_branch, plus_branch).dispatch_on_kind(&TABLE).parse_input(inp)
}
assert_eq!(Parser::with_parser(choose).parse_str("8").unwrap(), 8);
assert_eq!(Parser::with_parser(choose).parse_str("+").unwrap(), 0);
assert!(Parser::with_parser(choose).parse_str(";").is_err()); // `;` is in no table slot
```

---

## Repetition & folding

The [`repeated`](crate::try_parse_input::TryParseInput::repeated) driver runs a `TryParseInput`
element until it declines; [`repeated_while`](crate::ParseInput::repeated_while) runs a
plain `ParseInput` element until *your* peek-window condition says
[`Stop`](crate::parser::Action). [`collect`](crate::Accumulator::collect) accumulates the
elements into any [`Container`](crate::container::Container) (a `Vec`, a bounded array, …). The
**fold** family combines elements without an intermediate container.

| Combinator | On | One-liner |
|------------|-----|-----------|
| [`repeated()`](crate::try_parse_input::TryParseInput::repeated) | `TryParseInput` | repeat until the element declines |
| [`repeated_while(cond)`](crate::ParseInput::repeated_while) | `ParseInput` | repeat while `cond` (a peek decision) returns `Continue` |
| [`collect()`](crate::Accumulator::collect) / [`collect_with(c)`](crate::Accumulator::collect_with) | repetition | gather elements into a `Container` (default / provided) |
| [`fold(init, acc)`](crate::try_parse_input::TryParseInput::fold) | `TryParseInput` | left-fold: `acc: FnMut(O, O) -> O` |
| [`try_fold(init, acc)`](crate::try_parse_input::TryParseInput::try_fold) | `TryParseInput` | left-fold with a fallible `acc` |
| [`rfold(init, acc)`](crate::try_parse_input::TryParseInput::rfold) | `TryParseInput` | right-fold (buffers, `alloc`) |
| [`fold_while(cond, init, acc)`](crate::ParseInput::fold_while) | `ParseInput` | left-fold under a peek condition (also `try_fold_while`, `rfold_while`) |

```text
repeated(self) -> Repeated<…>                          // element: TryParseInput
collect(self) -> Collect<…>                            // on a repetition/separation driver
fold<Init, Acc>(self, init: Init, acc: Acc) -> Fold<…> // Init: FnMut() -> O, Acc: FnMut(O, O) -> O
```

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Accumulator as _, Parse, ParseInput as _, Parser, TryParseInput as _};
use tokora::try_parse_input::ParseAttempt;

fn try_digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<ParseAttempt<u32>, Error> {
    Ok(match inp.try_expect(|t| matches!(t.data(), Tok::Digit(_)))? {
        Some(sp) => match sp.into_data() { Tok::Digit(n) => ParseAttempt::Accept(n), _ => unreachable!() },
        None => ParseAttempt::Decline,
    })
}

// `repeated().collect()`: zero or more digits into a `Vec`.
fn digits<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<u32>, Error> {
    try_digit.repeated().collect().parse_input(inp)
}
assert_eq!(Parser::with_parser(digits).parse_str("123").unwrap(), vec![1, 2, 3]);

// `fold`: sum the digits without an intermediate container.
fn sum<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    try_digit.fold(|| 0, |acc, n| acc + n).parse_input(inp)
}
assert_eq!(Parser::with_parser(sum).parse_str("123").unwrap(), 6);
```

---

## Separation — comma-separated and friends

[`separated`](crate::try_parse_input::TryParseInput::separated) drives a declining element between
a typed separator [`Punctuator`](crate::punct::Punctuator); the ready-made spellings
[`separated_by_comma`](crate::try_parse_input::TryParseInput::separated_by_comma) and its family
(`separated_by_semicolon`, `_colon`, `_pipe`, … 18 in all) fix the separator. Wire your token to
the vocabulary with one [`PunctuatorToken`](crate::token::PunctuatorToken) impl (which kind is the
comma) and a `From<Comma<(), (), ()>>` for your kind (so the punctuator can name itself). When your
element cannot decline, [`separated_while`](crate::ParseInput::separated_while) (and the
`separated_by_*_while` spellings) take an explicit peek condition instead.

```text
separated<Sep>(self) -> Separated<…>                   // Sep: Punctuator; element: TryParseInput
separated_by_comma(self) -> Separated<Self, Comma, …>  // + _semicolon/_colon/_pipe/… (18)
separated_while<Sep, Cond, W>(self, cond: Cond) -> SeparatedWhile<…>  // element: ParseInput
```

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Accumulator as _, Parse, ParseInput as _, Parser, TryParseInput as _};
use tokora::try_parse_input::ParseAttempt;

fn try_digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<ParseAttempt<u32>, Error> {
    Ok(match inp.try_expect(|t| matches!(t.data(), Tok::Digit(_)))? {
        Some(sp) => match sp.into_data() { Tok::Digit(n) => ParseAttempt::Accept(n), _ => unreachable!() },
        None => ParseAttempt::Decline,
    })
}

// `separated_by_comma`: a comma list of digits.
fn csv<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<u32>, Error> {
    try_digit.separated_by_comma().collect().parse_input(inp)
}
assert_eq!(Parser::with_parser(csv).parse_str("1,2,3").unwrap(), vec![1, 2, 3]);
assert!(Parser::with_parser(csv).parse_str("1,,2").is_err()); // a doubled separator is a structured failure
```

---

## The `many/` builder surface

Each repetition/separation driver ([`Repeated`](crate::parser::Repeated),
[`Separated`](crate::parser::Separated),
[`SeparatedWhile`](crate::parser::SeparatedWhile)) exposes a small builder to tune element counts,
separator policy, and delimiters before you [`collect`](crate::Accumulator::collect). The
knobs return wrapper types (`AtLeast`, `AllowTrailing`, `Bounded`, …) that themselves chain, so
order is flexible.

| Knob | On | Effect |
|------|-----|--------|
| [`at_least(n)`](crate::parser::Separated::at_least) | both | require at least `n` elements — else a [`TooFew`](crate::error::syntax::TooFew) |
| [`at_most(n)`](crate::parser::Separated::at_most) | both | allow at most `n` — else a [`TooMany`](crate::error::syntax::TooMany) |
| [`bounded(min, max)`](crate::parser::Separated::bounded) | both | both bounds at once |
| [`allow_trailing()`](crate::parser::Separated::allow_trailing) | separated | accept a trailing separator |
| [`require_trailing()`](crate::parser::Separated::require_trailing) | separated | require a trailing separator |
| [`allow_leading()`](crate::parser::Separated::allow_leading) | separated | accept a leading separator |
| [`require_leading()`](crate::parser::Separated::require_leading) | separated | require a leading separator |
| [`delimited::<D>()`](crate::parser::Separated::delimited) | both | wrap in a [`Delimiter`](crate::delimiter::Delimiter) pair (`Paren`/`Bracket`/`Brace`/`Angle`); an unterminated list reports the opener as [`Unclosed`](crate::error::Unclosed) through [`UnclosedEmitter`](crate::emitter::UnclosedEmitter); for a single region see the [`delimited`](crate::parser::delimited)/[`parens`](crate::parser::parens) free shapes |

Trailing/leading violations report through dedicated emitter capabilities
([`UnexpectedTrailingSeparatorEmitter`](crate::emitter::UnexpectedTrailingSeparatorEmitter), …).
The **separator and delimiter hooks** —
[`SeparatorHandler`](crate::parser::SeparatorHandler) and
[`DelimiterHandler`](crate::parser::DelimiterHandler) — are how a container observes the
separators/brackets it stepped over: they are blanket-implemented as no-ops for every standard
container (`Vec`, `GenericArrayDeque`, `heapless`, `smallvec`, `tinyvec`), so a plain `collect()`
never has to mention them. Implement them on a custom accumulator to retain separator spans.

```text
// on Separated (also Repeated, minus the separator knobs)
at_least(self, n: usize)        allow_trailing(self)     allow_leading(self)
at_most(self, n: usize)         require_trailing(self)   require_leading(self)
bounded(self, min, max)         delimited::<Delim>(self) -> DelimitedBy<Self, Delim>
```

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Accumulator as _, Parse, ParseInput as _, Parser, TryParseInput as _, punct::Bracket};
use tokora::try_parse_input::ParseAttempt;

fn try_digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<ParseAttempt<u32>, Error> {
    Ok(match inp.try_expect(|t| matches!(t.data(), Tok::Digit(_)))? {
        Some(sp) => match sp.into_data() { Tok::Digit(n) => ParseAttempt::Accept(n), _ => unreachable!() },
        None => ParseAttempt::Decline,
    })
}

// bounds + a trailing separator, on a comma list
fn bounded_csv<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<u32>, Error> {
    try_digit.separated_by_comma().allow_trailing().at_least(1).collect().parse_input(inp)
}
assert_eq!(Parser::with_parser(bounded_csv).parse_str("1,2,").unwrap(), vec![1, 2]);

// `delimited::<Bracket>()`: a `[ … ]`-wrapped run of elements (no separators)
fn bracketed<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<u32>, Error> {
    try_digit.repeated().delimited::<Bracket>().collect().parse_input(inp)
}
assert_eq!(Parser::with_parser(bracketed).parse_str("[1 2 3]").unwrap(), vec![1, 2, 3]);
// An unterminated list reports the opener as `Unclosed` through the emitter — a hard
// error under this fail-fast context; a recovering emitter (`Verbose`) records the
// diagnostic and yields the elements collected so far.
assert!(Parser::with_parser(bracketed).parse_str("[1 2").is_err());
```

---

## Ready-made list atoms

For the common one-liners there are free functions (`alloc`/`std`) that assemble the drivers for
you and collect into a `Vec`:

| Atom | One-liner |
|------|-----------|
| [`separated1::<Sep, …>(item, peek)`](crate::parser::separated1) | one-or-more `item`s separated by `Sep`, optional leading separator |
| [`list_of(item, until)`](crate::parser::list_of) | zero-or-more `item`s until `until` accepts the next token (left in place) |
| [`try_ident_list::<Sep, …>()`](crate::parser::try_ident_list) | a separated list of identifiers into an [`IdentList`](crate::types::IdentList) (needs [`IdentifierToken`](crate::token::IdentifierToken)) |

One convention governs the free atoms, here and in [*Delimited shapes*](#delimited-shapes)
below: an atom takes its sub-parser as `impl` [`ParseInput`](crate::ParseInput) — a closure, a
fn item, or any named implementor ([`opt`](crate::parser::opt) takes an `impl`
[`TryParseInput`](crate::TryParseInput) attempt) — and hands back a builder-form closure that
is itself a `ParseInput` through the blanket impl, so atoms nest into each other and into the
method combinators without adapters. Predicate parameters — `peek`, `until`, and friends,
which inspect a token and answer `bool` — are functions, not parsers, and stay plain closures.

```text
separated1<Sep, …>(item: P, peek: Peek) -> impl FnMut(&mut InputRef) -> Result<Vec<T>, Error>
list_of<…>(item: P, until: Until) -> impl FnMut(&mut InputRef) -> Result<Vec<T>, Error>
```

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, Parser, parser::{list_of, separated1}};

fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    match inp.next()? { Some(sp) => match sp.into_data() { Tok::Digit(n) => Ok(n), _ => Err(Error) }, None => Err(Error) }
}

// `separated1`: one-or-more comma-separated digits (optional leading comma).
fn list<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<u32>, Error> {
    separated1::<Comma, _, _, _, _, _, _>(digit, |t| matches!(t, Tok::Digit(_)))(inp)
}
assert_eq!(Parser::with_parser(list).parse_str(",1,2,3").unwrap(), vec![1, 2, 3]);

// `list_of`: zero-or-more digits until the `]` (which is left in place).
fn run<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<u32>, Error> {
    list_of(digit, |t| matches!(t, Tok::RBracket))(inp)
}
assert_eq!(Parser::with_parser(run).parse_str("123]").unwrap(), vec![1, 2, 3]);
```

---

## Delimited shapes

A committed single-region shape: commit the opener, run the inner sub-parser, commit the
closer, and return a span-carrying [`Delimited`](crate::utils::Delimited) — the open value,
the close value, the inner output, and the whole-construct span. A missing closer reports the
opener as [`Unclosed`](crate::error::Unclosed) through the emitter — the same four-way
close-miss law the many-builders follow: end of input with the opener still open fires
`Unclosed` (a fail-fast emitter turns it into `Err`, a recovering one records it and yields the
construct recovered with a synthesized closer); a wrong token where the closer belongs stays
the unexpected-token (expected-close) diagnostic; a terminal scanner stop surfaces the
committed form's end-of-input error. This family fires only `Unclosed`, never the
`Unopened`/`Undelimited` half of the recovery vocabulary (see
[*Error taxonomy*](#error-taxonomy)).

[`delimited::<D, …>(inner)`](crate::parser::delimited) takes the delimiter pair as its first
type parameter through the [`TypedDelimiter`](crate::delimiter::TypedDelimiter) capability;
[`parens`](crate::parser::parens)/[`braces`](crate::parser::braces)/[`brackets`](crate::parser::brackets)/[`angles`](crate::parser::angles)
fix that pair to a built-in, and `parens(inner)` ≡ `delimited::<Paren, …>(inner)` for any
vocabulary whose two capability declarations agree. Bring your own pair by implementing
[`TypedDelimiter`](crate::delimiter::TypedDelimiter) for it. This is the single-region
counterpart to the many-builder's [`delimited::<D>()`](crate::parser::Separated::delimited),
which instead wraps a *repetition* and hands its delimiter tokens to a handler.

Every shape has an **attempt twin** that declines — `Ok(None)`, zero consumption — **iff the
opener is absent**: a wrong token or end of input at entry. The moment the opener is consumed
the parse is committed and every later diagnostic behaves exactly as the committed form's — an
unterminated group reports the opener as `Unclosed` through the emitter, never a silent decline.
The attempt boundary is deliberately the opener alone, not the whole shape: `opt(parens(inner))`
would swallow an unclosed group into a decline, where `Ident<` at end of input must report
`Unclosed` rather than silently disappear.

| Atom | One-liner |
|------|-----------|
| [`delimited::<D, …>(inner)`](crate::parser::delimited) | one `D`-delimited region into a span-carrying `Delimited` |
| [`parens(inner)`](crate::parser::parens) | the same, fixed to `( … )` |
| [`braces(inner)`](crate::parser::braces) | the same, fixed to `{ … }` |
| [`brackets(inner)`](crate::parser::brackets) | the same, fixed to `[ … ]` |
| [`angles(inner)`](crate::parser::angles) | the same, fixed to `< … >` |
| [`try_delimited::<D, …>(inner)`](crate::parser::try_delimited) | the attempt twin: `Ok(None)` iff the opener is absent, committed once it is consumed |
| [`try_parens`](crate::parser::try_parens) / [`try_braces`](crate::parser::try_braces) / [`try_brackets`](crate::parser::try_brackets) / [`try_angles`](crate::parser::try_angles) | the named attempt twins, pair fixed |

```text
delimited<D, …>(inner: P) -> impl FnMut(&mut InputRef) -> Result<Delimited<Open, Close, T>, Error>
parens(inner) / braces(inner) / brackets(inner) / angles(inner) -> the same, with the pair fixed
try_delimited<D, …>(inner) / try_parens(inner) / … -> the same, wrapped in Option (None iff the opener is absent)
```

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
#   punct::{Comma, OpenBracket, CloseBracket, OpenParen, CloseParen, OpenBrace, CloseBrace, OpenAngle, CloseAngle, Semicolon},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
# #[derive(Debug, Clone, PartialEq)]
# enum Tok { Digit(u32), Comma, Semi, LParen, RParen, LBracket, RBracket, LBrace, RBrace, LAngle, RAngle }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum Kind { Digit, Comma, Semi, LParen, RParen, LBracket, RBracket, LBrace, RBrace, LAngle, RAngle }
# impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
# impl Token<'_> for Tok {
#   type Kind = Kind;
#   type Error = Infallible;
#   fn kind(&self) -> Kind { match self {
#     Tok::Digit(_) => Kind::Digit, Tok::Comma => Kind::Comma, Tok::Semi => Kind::Semi,
#     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen,
#     Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket,
#     Tok::LBrace => Kind::LBrace, Tok::RBrace => Kind::RBrace,
#     Tok::LAngle => Kind::LAngle, Tok::RAngle => Kind::RAngle } }
#   fn is_trivia(&self) -> bool { false }
# }
# impl PunctuatorToken<'_> for Tok {
#   fn comma() -> Option<Kind> { Some(Kind::Comma) }
#   fn semicolon() -> Option<Kind> { Some(Kind::Semi) }
#   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
#   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
#   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
#   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
#   fn open_brace() -> Option<Kind> { Some(Kind::LBrace) }
#   fn close_brace() -> Option<Kind> { Some(Kind::RBrace) }
#   fn open_angle() -> Option<Kind> { Some(Kind::LAngle) }
#   fn close_angle() -> Option<Kind> { Some(Kind::RAngle) }
# }
# impl From<Comma<(), (), ()>> for Kind { fn from(_: Comma<(), (), ()>) -> Self { Kind::Comma } }
# impl From<Semicolon<(), (), ()>> for Kind { fn from(_: Semicolon<(), (), ()>) -> Self { Kind::Semi } }
# impl From<OpenParen<(), (), ()>> for Kind { fn from(_: OpenParen<(), (), ()>) -> Self { Kind::LParen } }
# impl From<CloseParen<(), (), ()>> for Kind { fn from(_: CloseParen<(), (), ()>) -> Self { Kind::RParen } }
# impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
# impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
# impl From<OpenBrace<(), (), ()>> for Kind { fn from(_: OpenBrace<(), (), ()>) -> Self { Kind::LBrace } }
# impl From<CloseBrace<(), (), ()>> for Kind { fn from(_: CloseBrace<(), (), ()>) -> Self { Kind::RBrace } }
# impl From<OpenAngle<(), (), ()>> for Kind { fn from(_: OpenAngle<(), (), ()>) -> Self { Kind::LAngle } }
# impl From<CloseAngle<(), (), ()>> for Kind { fn from(_: CloseAngle<(), (), ()>) -> Self { Kind::RAngle } }
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
#       ',' => Tok::Comma, ';' => Tok::Semi,
#       '(' => Tok::LParen, ')' => Tok::RParen, '[' => Tok::LBracket, ']' => Tok::RBracket,
#       '{' => Tok::LBrace, '}' => Tok::RBrace, '<' => Tok::LAngle, '>' => Tok::RAngle,
#       c => Tok::Digit(c as u32),
#     }))
#   }
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
# fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
#     match inp.next()? { Some(sp) => match sp.into_data() { Tok::Digit(n) => Ok(n), _ => Err(Error) }, None => Err(Error) }
# }
use tokora::{Parse, Parser, punct::Paren, parser::{braces, delimited, parens, try_parens}};

// `parens` wraps ONE region and keeps the typed delimiter values and the whole-construct span.
fn in_parens<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<(u32, SimpleSpan), Error> {
    let d = parens(digit)(inp)?;
    Ok((*d.data(), d.span()))
}
let (value, span) = Parser::with_parser(in_parens).parse_str("(1)").unwrap();
assert_eq!(value, 1);
assert_eq!(span, SimpleSpan::new(0, 3));

// `braces` fixes the pair to `{ … }`.
fn in_braces<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    braces(digit)(inp).map(|d| *d.data())
}
assert_eq!(Parser::with_parser(in_braces).parse_str("{1}").unwrap(), 1);

// `parens(inner)` ≡ `delimited::<Paren, …>(inner)`.
fn via_generic<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    delimited::<Paren, _, _, _, _, _, _>(digit)(inp).map(|d| *d.data())
}
assert_eq!(Parser::with_parser(via_generic).parse_str("(1)").unwrap(), 1);

// The attempt twin declines with zero consumption when the opener is absent…
fn attempt<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Option<u32>, Error> {
    try_parens(digit)(inp).map(|d| d.map(|d| *d.data()))
}
assert_eq!(Parser::with_parser(attempt).parse_str("1").unwrap(), None);
// …but once `(` is consumed it is committed: an unterminated group errors, it
// does not decline.
assert!(Parser::with_parser(attempt).parse_str("(1").is_err());
```

---

## CST bracketing

The [`node`](crate::parser::node) family wraps everything a sub-parse commits into one syntax node
over the emitter's event channel — the lossless-CST building block (see the lossless-CST chapter,
and [`crate::cst`]). Because the event channel is defaulted to no-ops on the diagnostic emitters,
these compile and run **tree-lessly** over [`FatalContext`](crate::FatalContext): the wrap is inert
and the output is just the inner parser's value.

| Combinator | One-liner |
|------------|-----------|
| [`node(kind, p)`](crate::parser::node) | wrap `p`'s committed span in a node of `kind` (a `u16`); no node on decline/error |
| [`node_opt(kind, p)`](crate::parser::node_opt) | as `node`, over a declining `p`, yielding `Option` — a decline records no (empty) node |
| [`node_at(mark, kind, p)`](crate::parser::node_at) | retro-wrap: anchor the node at a caller-held [`EventMark`](crate::cst::event::EventMark) |

```text
node(kind: u16, p: P) -> Node<P>              // P: ParseInput  (or TryParseInput)
node_opt(kind: u16, p: P) -> NodeOpt<P>       // P: TryParseInput -> ParseInput<Option<O>>
node_at(mark: EventMark, kind: u16, p: P) -> NodeAt<P>
```

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, ParseInput as _, Parser, parser::{node, node_opt}};
use tokora::try_parse_input::ParseAttempt;

const NUMBER: u16 = 1;

fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    match inp.next()? { Some(sp) => match sp.into_data() { Tok::Digit(n) => Ok(n), _ => Err(Error) }, None => Err(Error) }
}
fn try_digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<ParseAttempt<u32>, Error> {
    Ok(match inp.try_expect(|t| matches!(t.data(), Tok::Digit(_)))? {
        Some(sp) => match sp.into_data() { Tok::Digit(n) => ParseAttempt::Accept(n), _ => unreachable!() },
        None => ParseAttempt::Decline,
    })
}

// tree-less over `Fatal`: the wrap is inert, the value flows through.
fn number<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    node(NUMBER, digit).parse_input(inp)
}
fn opt_number<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Option<u32>, Error> {
    node_opt(NUMBER, try_digit).parse_input(inp)
}
assert_eq!(Parser::with_parser(number).parse_str("5").unwrap(), 5);
assert_eq!(Parser::with_parser(opt_number).parse_str("+").unwrap(), None);
```

---

## Wrapping parsers

| Wrapper | One-liner |
|---------|-----------|
| [`recover(r)`](crate::ParseInput::recover) | on error, **rewind** and run `r` (`FnMut(inp, err) -> Result<O, E>`) from the start |
| [`inplace_recover(r)`](crate::ParseInput::inplace_recover) | on error, run `r` from the **error position** (no rewind) — panic-mode resync |
| [`skip_then_retry(class, pred)`](crate::ParseInput::skip_then_retry) | on error, [`sync_balanced`](crate::InputRef::sync_balanced) to a sync point and retry |
| [`padded()`](crate::ParseInput::padded) | skip surrounding trivia (also `padded_left`, `padded_right`) |
| [`labelled(name, p)`](crate::labelled) | stamp `p`'s diagnostics with a *"while parsing name"* context |

`recover`/`inplace_recover`/`skip_then_retry` are the error-recovery surface taught in
[chapter 8](super::ch08_recovery); [`labelled`](crate::labelled) feeds
[`Verbose`](crate::emitter::Verbose) diagnostics (see [chapter 7](super::ch07_diagnostics)) and is a
no-op over a non-collecting emitter.

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
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
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Error { fn from(_: Unclosed<D, S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# impl tokora::error::MaybeTerminal for Error {}
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
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, ParseInput as _, Parser, labelled};

fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    match inp.next()? { Some(sp) => match sp.into_data() { Tok::Digit(n) => Ok(n), _ => Err(Error) }, None => Err(Error) }
}

// `recover`: on failure, rewind and fall back to a default (0).
fn digit_or_zero<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    digit
        .recover(|_inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>, _err: Error| Ok(0u32))
        .parse_input(inp)
}
assert_eq!(Parser::with_parser(digit_or_zero).parse_str("7").unwrap(), 7);
assert_eq!(Parser::with_parser(digit_or_zero).parse_str("+").unwrap(), 0); // `+` isn't a digit → recover

// `labelled`: a diagnostic context (a no-op under the fail-fast `Fatal` emitter).
fn labelled_digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
    labelled("a digit", digit).parse_input(inp)
}
assert_eq!(Parser::with_parser(labelled_digit).parse_str("4").unwrap(), 4);
```

---

## Error taxonomy

Errors are organized by category under [`crate::error`]. A parser never dictates a concrete error
type: it emits the leaf types below, and your error enum absorbs the ones it uses via `From`
(exactly the `impl From<…> for Error` block hidden in every example above). Each carries a span;
each has an `*Of` alias fixing `Lang = ()`, and [`ErrorOf<'inp, L, Ctx, Lang>`](crate::ErrorOf) is
the shorthand for a context's error type.

| Category | Types (module) |
|----------|----------------|
| **Token** | [`UnexpectedToken`](crate::error::token::UnexpectedToken), [`MissingToken`](crate::error::token::MissingToken), [`SeparatedError`](crate::error::token::SeparatedError) (in [`error::token`](crate::error::token)) |
| **End of input** | [`UnexpectedEnd`](crate::error::UnexpectedEnd) with aliases `UnexpectedEot` / `UnexpectedEof` / `UnexpectedEos` |
| **Lexer** | [`UnknownLexeme`](crate::error::UnknownLexeme), [`Malformed`](crate::error::Malformed) (+ per-literal aliases), [`Invalid`](crate::error::Invalid), hex/unicode escape errors |
| **Syntax** | [`TooFew`](crate::error::syntax::TooFew), [`TooMany`](crate::error::syntax::TooMany), [`FullContainer`](crate::error::syntax::FullContainer), [`MissingSyntax`](crate::error::syntax::MissingSyntax), [`IncompleteSyntax`](crate::error::IncompleteSyntax) (in [`error::syntax`](crate::error::syntax)) |
| **Delimiter** | [`Unclosed`](crate::error::Unclosed), [`Unopened`](crate::error::Unopened), [`Undelimited`](crate::error::Undelimited), [`Unterminated`](crate::error::Unterminated) |
| **Incomplete** | [`Incomplete`](crate::error::Incomplete) — the never-recoverable partial-input signal (see [chapter 9](super::ch09_streaming)) |

The whole taxonomy and the emitter/context surface are covered in depth in the
[errors, emitters & context reference](super::ref_errors_emitters_context);
[chapter 7](super::ch07_diagnostics) is the guided tutorial.

---

## Feature matrix

Defaults to `std`. `docs.rs` builds `all-features`.

| Feature | Enables | Notes |
|---------|---------|-------|
| `std` *(default)* | `std` library, all std-only backends | implies nothing else you must set |
| `alloc` | `Vec`/`String` drivers without `std` | the no-std + allocator tier |
| *(neither)* | core-only parsing | bounded containers only |
| `logos` (`= logos_0_16`) | the [`LogosLexer`](crate::lexer::LogosLexer) adapter | also `logos_0_14`, `logos_0_15` for other versions |
| `rowan` | the recording CST sink + typed lossless tree | implies `std`; the lossless-CST chapter |
| `unstable-raw` | the raw `InputRef::{save, restore, commit}` checkpoint triple | otherwise the transaction guards are the surface |
| `conformance` | the `conformance` lexer test kit | implies `std` |
| `fuzz` | the `fuzz` operation-script harness | implies `std` |
| `trace` | [`traced`](crate::traced) + combinator instrumentation | implies `std`; zero-cost when off |
| `bytes` / `bstr` / `hipstr` / `smol_bytes` | extra [`Source`](crate::Source)/[`Slice`](crate::Slice) backends | none implies `std`; each pins one upstream major |
| `smallvec` / `heapless` / `tinyvec` | extra [`Container`](crate::container::Container) backends | `smallvec` implies `alloc` |

Where a combinator needs a capability, its `where`-clause names it — e.g. the `many/` builder's
count checks want [`TooFewEmitter`](crate::emitter::TooFewEmitter) /
[`TooManyEmitter`](crate::emitter::TooManyEmitter) on the emitter. Over a
[`ParseCtx`](crate::ParseCtx) (any context whose emitter is a
[`ComposableEmitter`](crate::emitter::ComposableEmitter), including the built-in
[`Fatal`](crate::emitter::Fatal)/[`Verbose`](crate::emitter::Verbose)/[`Silent`](crate::emitter::Silent))
the whole family is available at once.
