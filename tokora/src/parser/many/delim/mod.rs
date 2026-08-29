use super::*;

mod repeated;
mod repeated_while;

/// A parser that wraps a repetition driver in a pair of delimiter tokens.
///
/// This combinator wraps any of the four repetition builders — [`Repeated`], [`RepeatedWhile`],
/// [`Separated`], [`SeparatedWhile`], and their bound/leading/trailing option wrappers — in
/// **opening and closing delimiters**, parsing constructs like `[element element element]` or
/// `{item, item, item}`.
///
/// The delimiter pair is a **type**, not a pair of classifier closures: `delimited::<Delim>()`
/// takes any [`Delimiter`](crate::delimiter::Delimiter), and `delimited_by_brackets` / `_braces` /
/// `_parens` / `_angles` name the built-in pairs.
///
/// # Type Parameters
///
/// - `P`: The wrapped repetition parser — which is what carries the element parser, the stopping
///   decision, the output type, the lookahead window, the lexer, the context and the language
/// - `Delim`: The delimiter pair marker (e.g. [`Bracket`](crate::punct::Bracket)), a
///   [`Delimiter`](crate::delimiter::Delimiter) whose `Open`/`Close` punctuators classify the two
///   tokens
///
/// # Examples
///
/// ## Basic Bracketed List
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
/// fn items<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num
///     .repeated_while(while_head(|t: &Tok| matches!(t, Tok::Num(_))))
///     .delimited_by_brackets()
///     .collect()
///     .parse_input(inp)
/// }
///
/// assert_eq!(Parser::with_parser(items).parse_str("[1 2 3]").unwrap(), vec![1, 2, 3]);
/// assert_eq!(Parser::with_parser(items).parse_str("[7]").unwrap(), vec![7]);
/// assert_eq!(Parser::with_parser(items).parse_str("[]").unwrap(), Vec::<i64>::new());
/// ```
///
/// ## Generic Delimiters
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
/// // Parse: {token token token}
/// fn items<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num
///     .repeated_while(while_head(|t: &Tok| matches!(t, Tok::Num(_))))
///     .delimited_by_braces()
///     .collect()
///     .parse_input(inp)
/// }
///
/// assert_eq!(Parser::with_parser(items).parse_str("{1 2 3}").unwrap(), vec![1, 2, 3]);
/// ```
///
/// ## Parenthesized Expressions
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
/// // Parse: (expr expr expr)
/// fn exprs<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num
///     .repeated_while(while_head(|t: &Tok| matches!(t, Tok::Num(_))))
///     .delimited_by_parens()
///     .collect()
///     .parse_input(inp)
/// }
///
/// assert_eq!(Parser::with_parser(exprs).parse_str("(1 2 3)").unwrap(), vec![1, 2, 3]);
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
/// # fn num<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<i64, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
/// #     Some(sp) => match sp.data { Tok::Num(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Accumulator, Parse, ParseInput, Parser, while_head};
///
/// // Parse 1-10 elements in brackets.
/// fn items<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Vec<i64>, Error> {
///   num
///     .repeated_while(while_head(|t: &Tok| matches!(t, Tok::Num(_))))
///     .at_least(1)
///     .at_most(10)
///     .delimited_by_brackets()
///     .collect()
///     .parse_input(inp)
/// }
///
/// assert_eq!(Parser::with_parser(items).parse_str("[7]").unwrap(), vec![7]);
/// // Below the minimum: the too-few diagnostic aborts under the fail-fast context.
/// assert!(Parser::with_parser(items).parse_str("[]").is_err());
/// ```
///
/// # How It Works
///
/// 1. **Parse opening delimiter**: Consume the left delimiter token
/// 2. **Parse elements**: Run the wrapped repetition driver
/// 3. **Parse closing delimiter**: Consume the right delimiter token
/// 4. **Return**: Return the collected elements
///
/// # Separated variants
///
/// The same wrapper carries a separated driver, so `[a, b, c]` is
/// `DelimitedBy<SeparatedWhile<..>, Bracket<..>>` — built by the identical `delimited_by_*`
/// call on a `separated`/`separated_while` builder.
///
/// | Feature | over a repeated driver | over a separated driver |
/// |---------|---------------|------------------------|
/// | **Separators** | No separators | Elements separated by a separator token |
/// | **Base Parser** | [`Repeated`] / [`RepeatedWhile`] | [`Separated`] / [`SeparatedWhile`] |
/// | **Example** | `[a b c]` | `[a, b, c]` |
/// | **Use Case** | Consecutive items | Separated lists |
///
/// # Performance
///
/// - **Memory**: O(1) for the parser structure
/// - **Runtime**: O(n) where n is the number of elements
/// - **Delimiter matching**: O(1) per delimiter
///
/// # See Also
///
/// - [`RepeatedWhile`] - One of the four repetition drivers this can wrap
/// - [`delimited`](RepeatedWhile::delimited) - How to create this combinator
/// - [`Collect`](crate::parser::Collect) - Wrapper for collecting elements into a container
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DelimitedBy<P, Delim> {
  pub(crate) parser: P,
  pub(crate) _delim: PhantomData<Delim>,
}

impl<P, Delim> DelimitedBy<P, Delim> {
  /// Creates a new `DelimitedBy` combinator wrapping the given parser.
  #[inline(always)]
  pub const fn new(parser: P) -> Self {
    Self {
      parser,
      _delim: PhantomData,
    }
  }

  /// Maps the inner parser via a mutable reference, returning a new `DelimitedBy`.
  #[inline(always)]
  pub fn map_parser_mut<'a, Q, F>(&'a mut self, f: F) -> DelimitedBy<Q, Delim>
  where
    F: FnOnce(&'a mut P) -> Q,
    Q: 'a,
  {
    DelimitedBy {
      parser: f(&mut self.parser),
      _delim: PhantomData,
    }
  }
}
