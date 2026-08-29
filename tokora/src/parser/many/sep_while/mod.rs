use core::marker::PhantomData;

use derive_more::IsVariant;

use crate::{delimiter::Delimiter, punct::Punctuator};

use super::*;

mod parse;

mod delim;

/// A parser that parses a sequence of elements separated by a separator token, under a
/// caller-supplied stopping condition.
///
/// This combinator parses repeated occurrences of an element parser, expecting each element to be
/// separated by a separator (e.g., comma, semicolon), and asks `condition` at each element
/// position whether to take another one. It provides fine-grained control over:
/// - **When to stop**: User-defined lookahead-based decision function
/// - **Leading separators**: Allow/deny/require separators before the first element
/// - **Trailing separators**: Allow/deny/require separators after the last element
/// - **Repetition bounds**: Minimum and maximum number of elements
///
/// # Type Parameters
///
/// - `F`: The element parser
/// - `Sep`: The separator [`Punctuator`](crate::punct::Punctuator) (e.g. [`Comma`](crate::punct::Comma))
/// - `Condition`: Decision function that determines when to stop parsing
/// - `O`: Output type of the element parser
/// - `Window`: Lookahead window size for the condition
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
/// - `Cmpl`: Completeness mode (default [`Complete`](crate::Complete))
///
/// # Examples
///
/// ## Basic Comma-Separated List
///
/// The condition is a [`Decision`](crate::Decision) over the **element** position — the separator
/// is already consumed by the time it is asked — so it classifies the token that starts the next
/// element, not the comma.
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
/// # fn num<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<i64, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
/// #     Some(sp) => match sp.data { Tok::Num(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{
///   Accumulator, EmitterView, Parse, ParseInput, Parser,
///   cache::{Peeked, PeekedTokenExt as _},
///   emitter::Fatal,
///   parser::Action,
///   punct::Comma,
///   utils::typenum::U1,
/// };
///
/// // Parse: element, element, element
/// fn while_num<'a>(
///   mut peeked: Peeked<'_, 'a, CharLexer<'a>, U1>,
///   _: EmitterView<'_, 'a, CharLexer<'a>, Fatal<Error>>,
/// ) -> Result<Action, Error> {
///   Ok(match peeked.pop_front() {
///     Some(t) if matches!(t.token(), Tok::Num(_)) => Action::Continue,
///     _ => Action::Stop,
///   })
/// }
///
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num.separated_while::<Comma, _, U1>(while_num).collect().parse_input(inp)
/// }
///
/// let got = Parser::with_parser(numbers).parse_str("1, 2, 3").unwrap();
/// assert_eq!(got, vec![1, 2, 3]);
/// ```
///
/// ## With Trailing Separator
///
/// [`while_kind`](crate::while_kind) is the width-1 adapter that continues while the head token's
/// kind matches, so the window parameter infers and the turbofish disappears.
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
/// # fn num<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<i64, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
/// #     Some(sp) => match sp.data { Tok::Num(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Accumulator, Parse, ParseInput, Parser, while_kind};
///
/// // Parse: element, element, element,  (trailing comma allowed)
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num
///     .separated_by_comma_while(while_kind(Kind::Num))
///     .allow_trailing()
///     .collect()
///     .parse_input(inp)
/// }
///
/// let got = Parser::with_parser(numbers).parse_str("1, 2, 3,").unwrap();
/// assert_eq!(got, vec![1, 2, 3]);
/// ```
///
/// ## With Leading Separator
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
/// # fn num<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<i64, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
/// #     Some(sp) => match sp.data { Tok::Num(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Accumulator, Parse, ParseInput, Parser, while_kind};
///
/// // Parse: , element, element  (leading comma allowed)
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num
///     .separated_by_comma_while(while_kind(Kind::Num))
///     .allow_leading()
///     .collect()
///     .parse_input(inp)
/// }
///
/// let got = Parser::with_parser(numbers).parse_str(", 1, 2").unwrap();
/// assert_eq!(got, vec![1, 2]);
/// ```
///
/// ## With Bounds
///
/// `SeparatedWhile` sets both bounds in one call. (`at_least` and `at_most` each exist on their
/// own, but unlike [`Repeated`] and [`RepeatedWhile`] they do not chain into a `Bounded`.)
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
/// # fn num<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<i64, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
/// #     Some(sp) => match sp.data { Tok::Num(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Accumulator, Parse, ParseInput, Parser, while_kind};
///
/// // Parse at least 1, at most 5 elements.
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num
///     .separated_by_comma_while(while_kind(Kind::Num))
///     .bounded(1, 5)
///     .collect()
///     .parse_input(inp)
/// }
///
/// let got = Parser::with_parser(numbers).parse_str("1, 2, 3").unwrap();
/// assert_eq!(got, vec![1, 2, 3]);
/// ```
///
/// ## Custom Separator
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
/// # fn num<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<i64, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
/// #     Some(sp) => match sp.data { Tok::Num(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Accumulator, Parse, ParseInput, Parser, while_kind};
///
/// // Parse elements separated by semicolons.
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num
///     .separated_by_semicolon_while(while_kind(Kind::Num))
///     .collect()
///     .parse_input(inp)
/// }
///
/// let got = Parser::with_parser(numbers).parse_str("1;2;3").unwrap();
/// assert_eq!(got, vec![1, 2, 3]);
/// ```
///
/// # How It Works
///
/// 1. **Parse first element** (unless a leading separator is required)
/// 2. **Loop**:
///    - Take a separator if one is at the front
///    - Otherwise call `condition` with a `W`-token window over the *element* position
///    - If `Action::Continue`: parse the next element
///    - If `Action::Stop`: break
/// 3. **Validate** leading/trailing separator rules and min/max bounds
/// 4. **Collect** parsed elements into container
///
/// The window the condition sees sits at the **element** position, not the separator one: the
/// separator is consumed before the decision is asked for. A condition that continues on the
/// separator token therefore stops immediately, on the very first element.
///
/// # Error Handling
///
/// The parser emits errors via the [`SeparatedEmitter`](crate::emitter::SeparatedEmitter) trait:
/// - Missing separator between elements
/// - Unexpected leading separator (when denied)
/// - Unexpected trailing separator (when denied)
/// - Missing element after separator
/// - Too few or too many elements (when bounds set)
///
/// # Performance
///
/// - **Memory**: O(1) for the parser itself (elements collected into container)
/// - **Parsing**: O(n) where n is the number of elements
/// - **Lookahead**: O(W) per iteration where W is the window size
///
/// # See Also
///
/// - [`delimited`](SeparatedWhile::delimited) - Wrap in delimiters (e.g., `[...]` or `{...}`)
/// - [`RepeatedWhile`] - Repeat without separators
/// - [`Separated`] - Separate until the element itself declines
/// - [`Collect`](crate::parser::Collect) - Wrapper for collecting elements into a container
///
/// # Completeness (0.3.0): Complete-only
///
/// Decision-window class: the `Decision` peeks a
/// `W`-window, and at a non-final Partial frontier the peek fill silently serves a SHORT
/// window (the peek contract: short at the frontier, never an error). The condition would
/// read that truncation as "construct ended" and return `Ok` early — breaking chunked
/// equivalence with no error on any channel. Generalizing needs the deferred
/// frontier-window rule (full-or-incomplete decision windows); until then the impls stay
/// pinned at `Complete` in both positions, so a Partial drive is a compile-time wall.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct SeparatedWhile<F, Sep, Condition, O, Window, L, Ctx, Lang: ?Sized = (), Cmpl = Complete>
{
  pub(super) f: F,
  pub(super) condition: Condition,
  pub(super) _sep: PhantomData<Sep>,
  pub(super) _m: PhantomData<O>,
  pub(super) _decision_window: PhantomData<Window>,
  pub(super) _l: PhantomData<L>,
  pub(super) _ctx: PhantomData<Ctx>,
  pub(super) _lang: PhantomData<Lang>,
  pub(super) _cmpl: PhantomData<Cmpl>,
}

impl<F, Sep, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl> Copy
  for SeparatedWhile<F, Sep, Condition, O, W, L, Ctx, Lang, Cmpl>
where
  F: Copy,
  Sep: Copy,
  Condition: Copy,
{
}

impl<F, Sep, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl> Clone
  for SeparatedWhile<F, Sep, Condition, O, W, L, Ctx, Lang, Cmpl>
where
  F: Clone,
  Sep: Clone,
  Condition: Clone,
{
  #[inline(always)]
  fn clone(&self) -> Self {
    Self {
      f: self.f.clone(),
      condition: self.condition.clone(),
      _sep: PhantomData,
      _m: PhantomData,
      _decision_window: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  SeparatedWhile<F, (), Condition, O, W, L, Ctx, Lang, Cmpl>
{
  /// Creates a new `SeparatedWhile` parser with the given container.
  #[inline(always)]
  pub const fn new<Sep>(
    f: F,
    condition: Condition,
  ) -> SeparatedWhile<F, Sep, Condition, O, W, L, Ctx, Lang, Cmpl> {
    SeparatedWhile {
      f,
      condition,
      _sep: PhantomData,
      _m: PhantomData,
      _decision_window: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<F, Sep, Condition, O, Window, L, Ctx, Lang: ?Sized, Cmpl>
  SeparatedWhile<F, Sep, Condition, O, Window, L, Ctx, Lang, Cmpl>
{
  /// Creates a mutable reference version of this `SeparatedWhile` parser.
  #[inline(always)]
  pub const fn as_mut(
    &mut self,
  ) -> SeparatedWhile<&mut F, Sep, &mut Condition, O, Window, L, Ctx, Lang, Cmpl> {
    SeparatedWhile {
      f: &mut self.f,
      condition: &mut self.condition,
      _sep: PhantomData,
      _m: PhantomData,
      _decision_window: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }

  /// Returns a mutable reference to the inner parser function.
  #[inline(always)]
  pub fn fn_mut(&mut self) -> &mut F {
    &mut self.f
  }

  /// Returns mutable references to the inner parser function and condition.
  #[inline(always)]
  pub fn parts_mut(&mut self) -> (&mut F, &mut Condition) {
    (&mut self.f, &mut self.condition)
  }

  /// Sets the minimum number of elements to parse.
  #[inline(always)]
  pub const fn at_least(self, minimum: usize) -> AtLeast<Self> {
    AtLeast::new(self, minimum)
  }

  /// Sets the maximum number of elements to parse.
  #[inline(always)]
  pub const fn at_most(self, maximum: usize) -> AtMost<Self> {
    AtMost::new(self, maximum)
  }

  /// Sets both the minimum and maximum number of elements to parse.
  #[inline(always)]
  pub const fn bounded(self, minimum: usize, maximum: usize) -> Bounded<Self> {
    Bounded::new(self, maximum, minimum)
  }

  /// Sets allows trailing separator.
  #[inline(always)]
  pub const fn allow_trailing(self) -> AllowTrailing<Self> {
    AllowTrailing::new(self)
  }

  /// Sets requires trailing separator.
  #[inline(always)]
  pub const fn require_trailing(self) -> RequireTrailing<Self> {
    RequireTrailing::new(self)
  }

  /// Sets allows leading separator.
  #[inline(always)]
  pub const fn allow_leading(self) -> AllowLeading<Self> {
    AllowLeading::new(self)
  }

  /// Sets requires leading separator.
  #[inline(always)]
  pub const fn require_leading(self) -> RequireLeading<Self> {
    RequireLeading::new(self)
  }

  define_many_delimited_methods!(Lang);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IsVariant)]
pub(super) enum State<T, S> {
  Start,
  Element,
  Leading(Spanned<T, S>),
  Separator(Spanned<T, S>),
}
