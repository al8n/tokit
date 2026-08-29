use core::marker::PhantomData;

use crate::emitter::FullContainerEmitter;

use super::*;

mod at_least;
mod at_most;
mod bounded;
mod unbounded;

/// A parser that repeatedly applies an element parser until a caller-supplied condition
/// signals to stop.
///
/// This combinator repeatedly parses elements **without separators** until the `condition`
/// returns [`Action::Stop`]. It provides fine-grained control over:
/// - **When to stop**: User-defined lookahead-based decision function
/// - **Repetition bounds**: Minimum and maximum number of elements
/// - **Delimiters**: Can wrap in delimiters like `[...]` or `{...}`
///
/// Unlike [`SeparatedWhile`], which expects a separator token between elements, `RepeatedWhile`
/// parses consecutive elements with nothing between them.
///
/// # Type Parameters
///
/// - `F`: The element parser
/// - `Condition`: Decision function that determines when to stop parsing (receives lookahead)
/// - `O`: Output type of the element parser
/// - `W`: Lookahead window size for the condition
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
/// - `Cmpl`: Completeness mode (default [`Complete`](crate::Complete))
///
/// # Examples
///
/// ## Basic Repetition
///
/// The condition is a [`Decision`](crate::Decision): it is handed a `W`-token window and an
/// [`EmitterView`](crate::EmitterView), and answers [`Action`]. A named function keeps the
/// closure's higher-ranked lifetimes out of the way.
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
///   utils::typenum::U1,
/// };
///
/// // Continue while the next token is a number; stop at anything else, and at end of input.
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
///   num.repeated_while::<_, U1>(while_num).collect().parse_input(inp)
/// }
///
/// let got = Parser::with_parser(numbers).parse_str("123 456 789 abc").unwrap();
/// assert_eq!(got, vec![123, 456, 789]);
/// ```
///
/// ## With Bounds
///
/// [`while_head`](crate::while_head) is the width-1 adapter for exactly the condition above,
/// so the window parameter infers and the turbofish disappears.
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
/// use tokora::{Accumulator, Parse, ParseInput, Parser, while_head};
///
/// // Parse at least 1, at most 10 elements.
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num
///     .repeated_while(while_head(|t: &Tok| matches!(t, Tok::Num(_))))
///     .at_least(1)
///     .at_most(10)
///     .collect()
///     .parse_input(inp)
/// }
///
/// assert_eq!(Parser::with_parser(numbers).parse_str("1 2 3").unwrap(), vec![1, 2, 3]);
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
/// # fn num<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<i64, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
/// #     Some(sp) => match sp.data { Tok::Num(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Accumulator, Parse, ParseInput, Parser, while_head};
///
/// // Parse: [element element element]
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num
///     .repeated_while(while_head(|t: &Tok| matches!(t, Tok::Num(_))))
///     .delimited_by_brackets()
///     .collect()
///     .parse_input(inp)
/// }
///
/// let got = Parser::with_parser(numbers).parse_str("[1 2 3 4]").unwrap();
/// assert_eq!(got, vec![1, 2, 3, 4]);
/// ```
///
/// ## Stop on Specific Token
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
///   utils::typenum::U1,
/// };
///
/// // Parse tokens until we see a semicolon — which is left in place for the caller.
/// fn until_semi<'a>(
///   mut peeked: Peeked<'_, 'a, CharLexer<'a>, U1>,
///   _: EmitterView<'_, 'a, CharLexer<'a>, Fatal<Error>>,
/// ) -> Result<Action, Error> {
///   Ok(match peeked.pop_front() {
///     Some(t) if matches!(t.token(), Tok::Semi) => Action::Stop,
///     None => Action::Stop,
///     _ => Action::Continue,
///   })
/// }
///
/// fn numbers<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num.repeated_while::<_, U1>(until_semi).collect().parse_input(inp)
/// }
///
/// let got = Parser::with_parser(numbers).parse_str("1 2 3 ;").unwrap();
/// assert_eq!(got, vec![1, 2, 3]);
/// ```
///
/// # How It Works
///
/// 1. **Loop**:
///    - Call `condition` with a `W`-token lookahead window to check whether to continue
///    - If `Action::Continue`: parse the next element
///    - If `Action::Stop`: break
/// 2. **Validate** min/max bounds
/// 3. **Collect** parsed elements into container
///
/// # Difference from `SeparatedWhile`
///
/// | Feature | `RepeatedWhile` | `SeparatedWhile` |
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
/// - **Lookahead**: O(W) per iteration where W is the window size
///
/// # See Also
///
/// - [`SeparatedWhile`] - Parse elements with separators (e.g., commas)
/// - [`Repeated`] - Repeat until the element itself declines
/// - [`delimited`](RepeatedWhile::delimited) - Wrap in delimiters
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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RepeatedWhile<F, Condition, O, W, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  pub(super) f: F,
  pub(super) condition: Condition,
  _m: PhantomData<O>,
  _cap: PhantomData<W>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  /// Creates a new `RepeatedWhile` parser.
  #[inline(always)]
  pub(crate) const fn new(f: F, condition: Condition) -> Self {
    Self::new_in(f, condition)
  }

  /// Creates a new `RepeatedWhile` parser with the given container.
  #[inline(always)]
  const fn new_in(f: F, condition: Condition) -> Self {
    Self {
      f,
      condition,
      _m: PhantomData,
      _cap: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  define_many_delimited_methods!(Lang);
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  /// Sets the minimum number of elements to parse.
  #[inline(always)]
  pub fn at_least(
    self,
    n: usize,
  ) -> AtLeast<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>> {
    self.apply(Minimum::new(n))
  }

  /// Sets the maximum number of elements to parse.
  #[inline(always)]
  pub fn at_most(self, n: usize) -> AtMost<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>> {
    self.apply(Maximum::new(n))
  }

  /// Sets both the minimum and maximum number of elements to parse.
  #[inline(always)]
  pub fn bounded(
    self,
    min: usize,
    max: usize,
  ) -> Bounded<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>> {
    self.apply(With::new(Maximum::new(max), Minimum::new(min)))
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl> Apply<AtLeast<Self>>
  for RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  type Options = Minimum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> AtLeast<Self> {
    AtLeast::new(self, options.get())
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl> Apply<AtMost<Self>>
  for RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  type Options = Maximum;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> AtMost<Self> {
    AtMost::new(self, options.get())
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl> Apply<Bounded<Self>>
  for RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>
{
  type Options = With<Maximum, Minimum>;

  #[inline(always)]
  fn apply(self, options: Self::Options) -> Bounded<Self> {
    Bounded::new(self, options.primary.get(), options.secondary.get())
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  Apply<Bounded<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>>>
  for AtMost<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>>
{
  type Options = Minimum;

  #[inline(always)]
  fn apply(
    self,
    options: Self::Options,
  ) -> Bounded<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>> {
    Bounded::new(self.parser, self.maximum.get(), options.get())
  }
}

impl<F, Condition, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  Apply<Bounded<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>>>
  for AtLeast<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>>
{
  type Options = Maximum;

  #[inline(always)]
  fn apply(
    self,
    options: Self::Options,
  ) -> Bounded<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang, Cmpl>> {
    Bounded::new(self.parser, options.get(), self.minimum.get())
  }
}

impl<'inp, 'c, L, F, Condition, O, Ctx, Lang: ?Sized, W>
  RepeatedWhile<F, Condition, O, W, L, Ctx, Lang>
{
  fn parse<Container, RH>(
    &mut self,
    inp: &mut InputRef<'inp, 'c, L, Ctx, Lang>,
    container: &mut Container,
    rh: &RH,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    F: ParseInput<'inp, L, O, Ctx, Lang>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
    Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>,
    Ctx: ParseContext<'inp, L, Lang>,
    // The decision-window gate surfaces a mid-window terminal scanner stop as this end-of-input
    // error.
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Container: crate::container::Container<O>,
    RH: RepeatedHandler<'inp, 'c, O, L, Ctx, Lang>,
  {
    trace_event!(inp, "repeated_while");
    let anchor = inp.cursor().clone();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence exits below: comparing the live latch against it
    // keeps that witness attempt-relative. One offset clone per collection.
    let latch = inp.latch_snapshot();
    // The scanner-trip baseline for the gates below — PER COLLECTION, taken beside the latch
    // and deliberately unlike the per-element descent one. It answers the latch's question
    // through a monotone session counter that no rollback reaches, which is what an element
    // catching a stop inside an `attempt` of its own leaves behind. See
    // `many::absence_after_element` for why the two granularities differ.
    let scans = inp.scanner_trip_snapshot();
    let mut nums = 0;
    let mut full = false;

    loop {
      // The descent witness's baseline, taken once per CYCLE — which is once per element, since a
      // cycle runs at most one. It sits at the top of the body rather than beside the element
      // attempt so that BOTH of this cycle's absence exits can read the same value; that widens the
      // measured window by the decision peek and `decide`, neither of which can descend, so the
      // reading is the one the element attempt would have given. See `many::absence_after_element`
      // for why it is per element and not per collection, and why the `Action::Stop` exit — reached
      // before this cycle's element runs — is a constant `false` here by construction: the element
      // that could have caught a trip is then the PREVIOUS cycle's *accepting* one, which the
      // module docs exempt.
      let trips = inp.trip_snapshot();
      // A short decision window can be a genuine end of input, but one truncated by a terminal
      // scanner stop is not: surface the committed end-of-input error before anything else. `decide`
      // is fallible and emitting, and a zero-width element makes no progress that would re-lex the
      // frontier, so neither may run first — no call may preempt the terminal stop. `end` is the
      // committed watermark, captured before the peek (the fill never advances it).
      let end = inp.span().end();
      let (peeked, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;
      if terminal {
        return Err(UnexpectedEot::eot_of(end).into_terminal().into());
      }

      match self.condition.decide(peeked, emitter)? {
        Action::Stop => {
          // A stop concludes *absence*: "no more elements". The element's own lookahead can latch a
          // terminal scanner stop and still return `Ok` with a short window, leaving the pre-trip
          // tokens cached — this window is then served whole from that cache, so it carries no
          // terminal flag for the gate above to see and the condition reads a truncated view as the
          // end of the construct. Both never-recoverable witnesses are the chokepoint's, not this
          // loop's; the scanner half is attempt-relative against the entry snapshot, so an inherited
          // boundary is not mis-charged here.
          absence_after_element(inp, &latch, scans, trips)?;
          let span = inp.span_since(&anchor);
          rh.on_stop(nums, inp, &anchor)?;
          return Ok(span);
        }
        Action::Continue => {
          // The admission runs only once the element has actually, successfully parsed —
          // matching the try-driven `Repeated::parse`, which never sees `Ok(Accept(item))` for an
          // element that failed. Checking `nums == max` ahead of `parse_input` fired the maximum
          // hook for an element that was merely *about to be attempted*: a `Continue` decision at
          // the boundary paired with a failing parse then reported `TooMany` (or masked the real
          // error under a fail-fast emitter) for an element that was never parsed, contradicting
          // the parsed-element accounting convention `admit_element` establishes.
          let item = self.f.parse_input(inp)?;
          admit_element(rh, &mut nums, &mut full, container, item, inp, &anchor)?;
        }
      }

      // A `Continue` cycle that consumed nothing re-sees the same lookahead and would decide
      // `Continue` forever. The progress metric is committed consumption (`span().end()`), never the
      // cache-front cursor — a lookahead fill moves that across skipped trivia without consuming,
      // reading a zero-width element as false progress. `<=`, not `==`: the watermark cannot regress
      // within a cycle, so anything not strictly ahead is a stall.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        // A stall concludes *absence*: "no more elements", on the strength of the element attempt
        // this cycle just ran. That attempt can hit either never-recoverable stop and still return
        // `Ok` — a terminal scanner stop its own lookahead latched (which the decision gate above
        // cannot see, because the latch happens after it), or a descent budget trip it caught
        // itself and answered with a value it consumed nothing for. Both are the chokepoint's; this
        // is the exit the measurement in `many`'s docs found spending the second of them.
        absence_after_element(inp, &latch, scans, trips)?;
        let span = inp.span_since(&anchor);
        rh.on_stop(nums, inp, &anchor)?;
        return Ok(span);
      }
      committed = new_committed;
    }
  }
}
