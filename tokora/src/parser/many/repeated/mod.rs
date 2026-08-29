use core::marker::PhantomData;

use crate::{
  TryParseInput,
  emitter::FullContainerEmitter,
  try_parse_input::{Accept, Decline},
};

use super::*;

mod at_least;
mod at_most;
mod bounded;
mod unbounded;

/// A parser that repeatedly applies a *try*-shaped element parser until it declines.
///
/// This combinator repeatedly parses elements **without separators**. The element itself owns
/// the stopping decision: it peeks, and returns
/// [`ParseAttempt::Decline`](crate::try_parse_input::ParseAttempt::Decline) — consuming nothing
/// — when the next token is not one of its own. On top of that it provides:
/// - **Repetition bounds**: Minimum and maximum number of elements
/// - **Delimiters**: Can wrap in delimiters like `[...]` or `{...}`
///
/// Unlike [`Separated`], which expects a separator token between elements, `Repeated` parses
/// consecutive elements with nothing between them. Unlike [`RepeatedWhile`], the stopping
/// decision is the element's own decline rather than a caller-supplied condition.
///
/// # Type Parameters
///
/// - `F`: The element parser — a [`TryParseInput`](crate::TryParseInput) whose decline ends the repetition
/// - `O`: Output type of the element parser
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
/// - `Cmpl`: Completeness mode (default [`Complete`](crate::Complete))
///
/// # Examples
///
/// ## Basic Repetition
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseBrace, CloseBracket, CloseParen, OpenBrace, OpenBracket, OpenParen, Semicolon},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Num(i64), Word, Comma, Semi, LBracket, RBracket, LBrace, RBrace, LParen, RParen }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Num, Word, Comma, Semi, LBracket, RBracket, LBrace, RBrace, LParen, RParen }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
/// #   fn kind(&self) -> Kind { match self {
/// #     Tok::Num(_) => Kind::Num, Tok::Word => Kind::Word,
/// #     Tok::Comma => Kind::Comma, Tok::Semi => Kind::Semi,
/// #     Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket,
/// #     Tok::LBrace => Kind::LBrace, Tok::RBrace => Kind::RBrace,
/// #     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
/// #   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
/// #   fn open_brace() -> Option<Kind> { Some(Kind::LBrace) }
/// #   fn close_brace() -> Option<Kind> { Some(Kind::RBrace) }
/// #   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
/// #   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
/// #   fn comma() -> Option<Kind> { Some(Kind::Comma) }
/// #   fn semicolon() -> Option<Kind> { Some(Kind::Semi) }
/// # }
/// # impl From<tokora::punct::Comma<(), (), ()>> for Kind { fn from(_: tokora::punct::Comma<(), (), ()>) -> Self { Kind::Comma } }
/// # impl From<Semicolon<(), (), ()>> for Kind { fn from(_: Semicolon<(), (), ()>) -> Self { Kind::Semi } }
/// # impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
/// # impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
/// # impl From<OpenBrace<(), (), ()>> for Kind { fn from(_: OpenBrace<(), (), ()>) -> Self { Kind::LBrace } }
/// # impl From<CloseBrace<(), (), ()>> for Kind { fn from(_: CloseBrace<(), (), ()>) -> Self { Kind::RBrace } }
/// # impl From<OpenParen<(), (), ()>> for Kind { fn from(_: OpenParen<(), (), ()>) -> Self { Kind::LParen } }
/// # impl From<CloseParen<(), (), ()>> for Kind { fn from(_: CloseParen<(), (), ()>) -> Self { Kind::RParen } }
/// # struct CharLexer<'a> { src: &'a str, pos: usize, tok: SimpleSpan, state: () }
/// # impl<'a> Lexer<'a> for CharLexer<'a> {
/// #   type State = (); type Source = str; type Token = Tok; type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'a str) -> Self { Self { src, pos: 0, tok: SimpleSpan::new(0, 0), state: () } }
/// #   fn with_state(src: &'a str, _: ()) -> Self { Self::new(src) }
/// #   fn check(&self) -> Result<(), Infallible> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> Self::State {}
/// #   fn source(&self) -> &'a str { self.src }
/// #   fn span(&self) -> SimpleSpan { self.tok }
/// #   fn slice(&self) -> &'a str { &self.src[self.tok.start()..self.tok.end()] }
/// #   fn lex(&mut self) -> Option<Result<Tok, Infallible>> {
/// #     let bytes = self.src.as_bytes();
/// #     while self.pos < bytes.len() && bytes[self.pos] == b' ' { self.pos += 1; }
/// #     if self.pos >= bytes.len() { return None; }
/// #     let start = self.pos;
/// #     let tok = if bytes[self.pos].is_ascii_digit() {
/// #       while self.pos < bytes.len() && bytes[self.pos].is_ascii_digit() { self.pos += 1; }
/// #       Tok::Num(self.src[start..self.pos].parse().unwrap())
/// #     } else {
/// #       self.pos += 1;
/// #       match bytes[start] {
/// #         b',' => Tok::Comma, b';' => Tok::Semi,
/// #         b'[' => Tok::LBracket, b']' => Tok::RBracket,
/// #         b'{' => Tok::LBrace, b'}' => Tok::RBrace,
/// #         b'(' => Tok::LParen, b')' => Tok::RParen,
/// #         _ => Tok::Word,
/// #       }
/// #     };
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(tok))
/// #   }
/// #   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # use tokora::try_parse_input::ParseAttempt;
/// # fn try_num<'a>(
/// #   inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
/// # ) -> Result<ParseAttempt<i64>, Error> {
/// #   Ok(match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
/// #     Some(sp) => match sp.data { Tok::Num(n) => ParseAttempt::Accept(n), _ => unreachable!() },
/// #     None => ParseAttempt::Decline,
/// #   })
/// # }
/// use tokora::{Accumulator, Parse, ParseInput, Parser, TryParseInput};
///
/// // Parse numbers until the element declines — here, at the first non-number token.
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   try_num.repeated().collect().parse_input(inp)
/// }
///
/// let got = Parser::with_parser(numbers).parse_str("123 456 789 abc").unwrap();
/// assert_eq!(got, vec![123, 456, 789]);
/// ```
///
/// ## With Bounds
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseBrace, CloseBracket, CloseParen, OpenBrace, OpenBracket, OpenParen, Semicolon},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Num(i64), Word, Comma, Semi, LBracket, RBracket, LBrace, RBrace, LParen, RParen }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Num, Word, Comma, Semi, LBracket, RBracket, LBrace, RBrace, LParen, RParen }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
/// #   fn kind(&self) -> Kind { match self {
/// #     Tok::Num(_) => Kind::Num, Tok::Word => Kind::Word,
/// #     Tok::Comma => Kind::Comma, Tok::Semi => Kind::Semi,
/// #     Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket,
/// #     Tok::LBrace => Kind::LBrace, Tok::RBrace => Kind::RBrace,
/// #     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
/// #   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
/// #   fn open_brace() -> Option<Kind> { Some(Kind::LBrace) }
/// #   fn close_brace() -> Option<Kind> { Some(Kind::RBrace) }
/// #   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
/// #   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
/// #   fn comma() -> Option<Kind> { Some(Kind::Comma) }
/// #   fn semicolon() -> Option<Kind> { Some(Kind::Semi) }
/// # }
/// # impl From<tokora::punct::Comma<(), (), ()>> for Kind { fn from(_: tokora::punct::Comma<(), (), ()>) -> Self { Kind::Comma } }
/// # impl From<Semicolon<(), (), ()>> for Kind { fn from(_: Semicolon<(), (), ()>) -> Self { Kind::Semi } }
/// # impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
/// # impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
/// # impl From<OpenBrace<(), (), ()>> for Kind { fn from(_: OpenBrace<(), (), ()>) -> Self { Kind::LBrace } }
/// # impl From<CloseBrace<(), (), ()>> for Kind { fn from(_: CloseBrace<(), (), ()>) -> Self { Kind::RBrace } }
/// # impl From<OpenParen<(), (), ()>> for Kind { fn from(_: OpenParen<(), (), ()>) -> Self { Kind::LParen } }
/// # impl From<CloseParen<(), (), ()>> for Kind { fn from(_: CloseParen<(), (), ()>) -> Self { Kind::RParen } }
/// # struct CharLexer<'a> { src: &'a str, pos: usize, tok: SimpleSpan, state: () }
/// # impl<'a> Lexer<'a> for CharLexer<'a> {
/// #   type State = (); type Source = str; type Token = Tok; type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'a str) -> Self { Self { src, pos: 0, tok: SimpleSpan::new(0, 0), state: () } }
/// #   fn with_state(src: &'a str, _: ()) -> Self { Self::new(src) }
/// #   fn check(&self) -> Result<(), Infallible> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> Self::State {}
/// #   fn source(&self) -> &'a str { self.src }
/// #   fn span(&self) -> SimpleSpan { self.tok }
/// #   fn slice(&self) -> &'a str { &self.src[self.tok.start()..self.tok.end()] }
/// #   fn lex(&mut self) -> Option<Result<Tok, Infallible>> {
/// #     let bytes = self.src.as_bytes();
/// #     while self.pos < bytes.len() && bytes[self.pos] == b' ' { self.pos += 1; }
/// #     if self.pos >= bytes.len() { return None; }
/// #     let start = self.pos;
/// #     let tok = if bytes[self.pos].is_ascii_digit() {
/// #       while self.pos < bytes.len() && bytes[self.pos].is_ascii_digit() { self.pos += 1; }
/// #       Tok::Num(self.src[start..self.pos].parse().unwrap())
/// #     } else {
/// #       self.pos += 1;
/// #       match bytes[start] {
/// #         b',' => Tok::Comma, b';' => Tok::Semi,
/// #         b'[' => Tok::LBracket, b']' => Tok::RBracket,
/// #         b'{' => Tok::LBrace, b'}' => Tok::RBrace,
/// #         b'(' => Tok::LParen, b')' => Tok::RParen,
/// #         _ => Tok::Word,
/// #       }
/// #     };
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(tok))
/// #   }
/// #   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # use tokora::try_parse_input::ParseAttempt;
/// # fn try_num<'a>(
/// #   inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
/// # ) -> Result<ParseAttempt<i64>, Error> {
/// #   Ok(match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
/// #     Some(sp) => match sp.data { Tok::Num(n) => ParseAttempt::Accept(n), _ => unreachable!() },
/// #     None => ParseAttempt::Decline,
/// #   })
/// # }
/// use tokora::{Accumulator, Parse, ParseInput, Parser, TryParseInput};
///
/// // Parse at least 1, at most 10 elements.
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   try_num.repeated().at_least(1).at_most(10).collect().parse_input(inp)
/// }
///
/// assert_eq!(Parser::with_parser(numbers).parse_str("1 2 3").unwrap(), vec![1, 2, 3]);
/// // Below the minimum: the too-few diagnostic aborts under the fail-fast context.
/// assert!(Parser::with_parser(numbers).parse_str("").is_err());
/// ```
///
/// ## Delimited Repetition
///
/// The delimiter pair is a **type**, not a pair of closures: `delimited::<Delim>()` takes any
/// [`Delimiter`](crate::delimiter::Delimiter), and `delimited_by_brackets` / `_braces` /
/// `_parens` / `_angles` name the built-in pairs.
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseBrace, CloseBracket, CloseParen, OpenBrace, OpenBracket, OpenParen, Semicolon},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Num(i64), Word, Comma, Semi, LBracket, RBracket, LBrace, RBrace, LParen, RParen }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Num, Word, Comma, Semi, LBracket, RBracket, LBrace, RBrace, LParen, RParen }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
/// #   fn kind(&self) -> Kind { match self {
/// #     Tok::Num(_) => Kind::Num, Tok::Word => Kind::Word,
/// #     Tok::Comma => Kind::Comma, Tok::Semi => Kind::Semi,
/// #     Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket,
/// #     Tok::LBrace => Kind::LBrace, Tok::RBrace => Kind::RBrace,
/// #     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
/// #   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
/// #   fn open_brace() -> Option<Kind> { Some(Kind::LBrace) }
/// #   fn close_brace() -> Option<Kind> { Some(Kind::RBrace) }
/// #   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
/// #   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
/// #   fn comma() -> Option<Kind> { Some(Kind::Comma) }
/// #   fn semicolon() -> Option<Kind> { Some(Kind::Semi) }
/// # }
/// # impl From<tokora::punct::Comma<(), (), ()>> for Kind { fn from(_: tokora::punct::Comma<(), (), ()>) -> Self { Kind::Comma } }
/// # impl From<Semicolon<(), (), ()>> for Kind { fn from(_: Semicolon<(), (), ()>) -> Self { Kind::Semi } }
/// # impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
/// # impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
/// # impl From<OpenBrace<(), (), ()>> for Kind { fn from(_: OpenBrace<(), (), ()>) -> Self { Kind::LBrace } }
/// # impl From<CloseBrace<(), (), ()>> for Kind { fn from(_: CloseBrace<(), (), ()>) -> Self { Kind::RBrace } }
/// # impl From<OpenParen<(), (), ()>> for Kind { fn from(_: OpenParen<(), (), ()>) -> Self { Kind::LParen } }
/// # impl From<CloseParen<(), (), ()>> for Kind { fn from(_: CloseParen<(), (), ()>) -> Self { Kind::RParen } }
/// # struct CharLexer<'a> { src: &'a str, pos: usize, tok: SimpleSpan, state: () }
/// # impl<'a> Lexer<'a> for CharLexer<'a> {
/// #   type State = (); type Source = str; type Token = Tok; type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'a str) -> Self { Self { src, pos: 0, tok: SimpleSpan::new(0, 0), state: () } }
/// #   fn with_state(src: &'a str, _: ()) -> Self { Self::new(src) }
/// #   fn check(&self) -> Result<(), Infallible> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> Self::State {}
/// #   fn source(&self) -> &'a str { self.src }
/// #   fn span(&self) -> SimpleSpan { self.tok }
/// #   fn slice(&self) -> &'a str { &self.src[self.tok.start()..self.tok.end()] }
/// #   fn lex(&mut self) -> Option<Result<Tok, Infallible>> {
/// #     let bytes = self.src.as_bytes();
/// #     while self.pos < bytes.len() && bytes[self.pos] == b' ' { self.pos += 1; }
/// #     if self.pos >= bytes.len() { return None; }
/// #     let start = self.pos;
/// #     let tok = if bytes[self.pos].is_ascii_digit() {
/// #       while self.pos < bytes.len() && bytes[self.pos].is_ascii_digit() { self.pos += 1; }
/// #       Tok::Num(self.src[start..self.pos].parse().unwrap())
/// #     } else {
/// #       self.pos += 1;
/// #       match bytes[start] {
/// #         b',' => Tok::Comma, b';' => Tok::Semi,
/// #         b'[' => Tok::LBracket, b']' => Tok::RBracket,
/// #         b'{' => Tok::LBrace, b'}' => Tok::RBrace,
/// #         b'(' => Tok::LParen, b')' => Tok::RParen,
/// #         _ => Tok::Word,
/// #       }
/// #     };
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(tok))
/// #   }
/// #   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # use tokora::try_parse_input::ParseAttempt;
/// # fn try_num<'a>(
/// #   inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
/// # ) -> Result<ParseAttempt<i64>, Error> {
/// #   Ok(match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
/// #     Some(sp) => match sp.data { Tok::Num(n) => ParseAttempt::Accept(n), _ => unreachable!() },
/// #     None => ParseAttempt::Decline,
/// #   })
/// # }
/// use tokora::{Accumulator, Parse, ParseInput, Parser, TryParseInput};
///
/// // Parse: [element element element]
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   try_num.repeated().delimited_by_brackets().collect().parse_input(inp)
/// }
///
/// let got = Parser::with_parser(numbers).parse_str("[1 2 3 4]").unwrap();
/// assert_eq!(got, vec![1, 2, 3, 4]);
/// ```
///
/// ## Stop on a Specific Token
///
/// `Repeated` has no condition hook of its own, so "stop at `;`" is expressed by the element:
/// it declines on the semicolon, which leaves that token in place for the caller.
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseBrace, CloseBracket, CloseParen, OpenBrace, OpenBracket, OpenParen, Semicolon},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Num(i64), Word, Comma, Semi, LBracket, RBracket, LBrace, RBrace, LParen, RParen }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Num, Word, Comma, Semi, LBracket, RBracket, LBrace, RBrace, LParen, RParen }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
/// #   fn kind(&self) -> Kind { match self {
/// #     Tok::Num(_) => Kind::Num, Tok::Word => Kind::Word,
/// #     Tok::Comma => Kind::Comma, Tok::Semi => Kind::Semi,
/// #     Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket,
/// #     Tok::LBrace => Kind::LBrace, Tok::RBrace => Kind::RBrace,
/// #     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
/// #   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
/// #   fn open_brace() -> Option<Kind> { Some(Kind::LBrace) }
/// #   fn close_brace() -> Option<Kind> { Some(Kind::RBrace) }
/// #   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
/// #   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
/// #   fn comma() -> Option<Kind> { Some(Kind::Comma) }
/// #   fn semicolon() -> Option<Kind> { Some(Kind::Semi) }
/// # }
/// # impl From<tokora::punct::Comma<(), (), ()>> for Kind { fn from(_: tokora::punct::Comma<(), (), ()>) -> Self { Kind::Comma } }
/// # impl From<Semicolon<(), (), ()>> for Kind { fn from(_: Semicolon<(), (), ()>) -> Self { Kind::Semi } }
/// # impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
/// # impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
/// # impl From<OpenBrace<(), (), ()>> for Kind { fn from(_: OpenBrace<(), (), ()>) -> Self { Kind::LBrace } }
/// # impl From<CloseBrace<(), (), ()>> for Kind { fn from(_: CloseBrace<(), (), ()>) -> Self { Kind::RBrace } }
/// # impl From<OpenParen<(), (), ()>> for Kind { fn from(_: OpenParen<(), (), ()>) -> Self { Kind::LParen } }
/// # impl From<CloseParen<(), (), ()>> for Kind { fn from(_: CloseParen<(), (), ()>) -> Self { Kind::RParen } }
/// # struct CharLexer<'a> { src: &'a str, pos: usize, tok: SimpleSpan, state: () }
/// # impl<'a> Lexer<'a> for CharLexer<'a> {
/// #   type State = (); type Source = str; type Token = Tok; type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'a str) -> Self { Self { src, pos: 0, tok: SimpleSpan::new(0, 0), state: () } }
/// #   fn with_state(src: &'a str, _: ()) -> Self { Self::new(src) }
/// #   fn check(&self) -> Result<(), Infallible> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> Self::State {}
/// #   fn source(&self) -> &'a str { self.src }
/// #   fn span(&self) -> SimpleSpan { self.tok }
/// #   fn slice(&self) -> &'a str { &self.src[self.tok.start()..self.tok.end()] }
/// #   fn lex(&mut self) -> Option<Result<Tok, Infallible>> {
/// #     let bytes = self.src.as_bytes();
/// #     while self.pos < bytes.len() && bytes[self.pos] == b' ' { self.pos += 1; }
/// #     if self.pos >= bytes.len() { return None; }
/// #     let start = self.pos;
/// #     let tok = if bytes[self.pos].is_ascii_digit() {
/// #       while self.pos < bytes.len() && bytes[self.pos].is_ascii_digit() { self.pos += 1; }
/// #       Tok::Num(self.src[start..self.pos].parse().unwrap())
/// #     } else {
/// #       self.pos += 1;
/// #       match bytes[start] {
/// #         b',' => Tok::Comma, b';' => Tok::Semi,
/// #         b'[' => Tok::LBracket, b']' => Tok::RBracket,
/// #         b'{' => Tok::LBrace, b'}' => Tok::RBrace,
/// #         b'(' => Tok::LParen, b')' => Tok::RParen,
/// #         _ => Tok::Word,
/// #       }
/// #     };
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(tok))
/// #   }
/// #   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// use tokora::{
///   Accumulator, Parse, ParseInput, Parser, TryParseInput, try_parse_input::ParseAttempt,
/// };
///
/// // Take tokens until we see a semicolon.
/// fn token<'a>(
///   inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
/// ) -> Result<ParseAttempt<Tok>, Error> {
///   Ok(match inp.try_expect(|t| !matches!(t.data, Tok::Semi))? {
///     Some(sp) => ParseAttempt::Accept(sp.data),
///     None => ParseAttempt::Decline,
///   })
/// }
///
/// fn tokens<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<Tok>, Error> {
///   token.repeated().collect().parse_input(inp)
/// }
///
/// let got = Parser::with_parser(tokens).parse_str("1 2 ; 3").unwrap();
/// assert_eq!(got, vec![Tok::Num(1), Tok::Num(2)]);
/// ```
///
/// # How It Works
///
/// 1. **Loop**:
///    - Run the element parser
///    - If it accepts: push the element and continue
///    - If it declines: break — nothing was consumed, so the token stays for the caller
/// 2. **Validate** min/max bounds
/// 3. **Collect** parsed elements into container
///
/// # Difference from `Separated`
///
/// | Feature | `Repeated` | `Separated` |
/// |---------|-----------|---------------|
/// | **Separators** | No separators | Elements separated by a separator token |
/// | **Use Case** | Consecutive elements | Comma/semicolon-separated lists |
/// | **Example** | `1 2 3 4` | `1, 2, 3, 4` |
///
/// # Error Handling
///
/// The parser emits errors via the traits:
/// - [`TooFewEmitter`](crate::emitter::TooFewEmitter): Too few elements (below minimum)
/// - [`TooManyEmitter`](crate::emitter::TooManyEmitter): Too many elements (above maximum)
///
/// # Performance
///
/// - **Memory**: O(1) for the parser itself (elements collected into container)
/// - **Parsing**: O(n) where n is the number of elements
///
/// # See Also
///
/// - [`Separated`] - Parse elements with separators (e.g., commas)
/// - [`RepeatedWhile`] - Repeat under a caller-supplied stopping condition
/// - [`delimited`](Repeated::delimited) - Wrap in delimiters
/// - [`Collect`](crate::parser::Collect) - Wrapper for collecting elements into a container
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Repeated<F, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  pub(super) f: F,
  _m: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Repeated<F, O, L, Ctx, Lang, Cmpl> {
  /// Creates a new `Repeated` parser.
  #[inline(always)]
  pub(crate) const fn new(f: F) -> Self {
    Self {
      f,
      _m: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Repeated<F, O, L, Ctx, Lang, Cmpl> {
  define_many_delimited_methods!(Lang);
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Repeated<F, O, L, Ctx, Lang, Cmpl> {
  /// Sets the minimum number of elements to parse.
  #[inline(always)]
  pub fn at_least(self, n: usize) -> AtLeast<Repeated<F, O, L, Ctx, Lang, Cmpl>> {
    self.apply(Minimum::new(n))
  }

  /// Sets the maximum number of elements to parse.
  #[inline(always)]
  pub fn at_most(self, n: usize) -> AtMost<Repeated<F, O, L, Ctx, Lang, Cmpl>> {
    self.apply(Maximum::new(n))
  }

  /// Sets both the minimum and maximum number of elements to parse.
  #[inline(always)]
  pub fn bounded(self, min: usize, max: usize) -> Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>> {
    self.apply(With::new(Maximum::new(max), Minimum::new(min)))
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Apply<AtLeast<Self>> for Repeated<F, O, L, Ctx, Lang, Cmpl> {
  type Options = Minimum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> AtLeast<Self> {
    AtLeast::new(self, options.get())
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Apply<AtMost<Self>> for Repeated<F, O, L, Ctx, Lang, Cmpl> {
  type Options = Maximum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> AtMost<Self> {
    AtMost::new(self, options.get())
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Apply<Bounded<Self>> for Repeated<F, O, L, Ctx, Lang, Cmpl> {
  type Options = With<Maximum, Minimum>;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> Bounded<Self> {
    Bounded::new(self, options.primary.get(), options.secondary.get())
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Apply<Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>>>
  for AtMost<Repeated<F, O, L, Ctx, Lang, Cmpl>>
{
  type Options = Minimum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>> {
    Bounded::new(self.parser, self.maximum.get(), options.get())
  }
}

impl<F, O, L, Ctx, Lang: ?Sized, Cmpl> Apply<Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>>>
  for AtLeast<Repeated<F, O, L, Ctx, Lang, Cmpl>>
{
  type Options = Maximum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>> {
    Bounded::new(self.parser, options.get(), self.minimum.get())
  }
}

impl<'inp, 'c, L, F, O, Ctx, Lang: ?Sized, Cmpl> Repeated<F, O, L, Ctx, Lang, Cmpl> {
  pub(super) fn parse<Container, RH>(
    &mut self,
    inp: &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
    container: &mut Container,
    rh: &RH,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Ctx::Emitter: Emitter<'inp, L, Lang> + FullContainerEmitter<'inp, L, Lang>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    // The absence-exit gate surfaces a terminal scanner stop as this end-of-input error.
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Container: crate::container::Container<O>,
    RH: RepeatedHandler<'inp, 'c, O, L, Ctx, Lang, Cmpl>,
  {
    trace_event!(inp, "repeated");
    let mut num = 0;
    let mut full = false;
    let anchor = inp.cursor().clone();
    let mut cursor = anchor.clone();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence exit below: comparing the live latch against it
    // keeps that witness attempt-relative. One offset clone per collection, off the per-element path.
    let latch = inp.latch_snapshot();
    // The scanner-trip baseline for the gates below — PER COLLECTION, taken beside the latch
    // and deliberately unlike the per-element descent one. It answers the latch's question
    // through a monotone session counter that no rollback reaches, which is what an element
    // catching a stop inside an `attempt` of its own leaves behind. See
    // `many::absence_after_element` for why the two granularities differ.
    let scans = inp.scanner_trip_snapshot();

    // The trip baseline of the LAST element attempt, carried out by whichever break concluded
    // absence — see `many::absence_after_element` for what the gate below does with it, and why the
    // value has to be carried rather than re-read after the loop.
    //
    // The cycle itself is `try_element_cycle!`, shared with `delim/repeated.rs`: descent baseline,
    // attempt, admission, failure chokepoint, stall test, bookkeeping. This driver's decline is the
    // bare `break trips` below; the delimited one puts its close probe in the same hole. Every local
    // the cycle touches is declared above, in this order, with this initializer — the macro declares
    // and moves nothing, which is what makes the expansion the text it replaced. See the macro for
    // why that is worth a macro.
    let elem_trips = try_element_cycle!(
      inp, self.f, rh, container,
      anchor: anchor,
      error_anchor: cursor,
      committed: committed,
      full: full,
      nums: num,
      scans: scans,
      trips: trips,
      on_decline: break trips,
    );

    // Both ways out of the loop above — the element declining, and a cycle that committed nothing —
    // conclude *absence*: "no more elements", on the strength of what the last element attempt did.
    // The chokepoint above never sees either, because neither produced an `Err`, so the same two
    // never-recoverable facts have to be witnessed here. `absence_after_element` holds both and says
    // why each baseline is the granularity it is. The end-of-input anchors on the committed end,
    // matching the decision-window and consume gates.
    absence_after_element(inp, &latch, scans, elem_trips)?;

    rh.on_stop(num, inp, &anchor)
  }
}
