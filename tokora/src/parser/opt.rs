use crate::{
  ComposableParseContext, ErrorOf, Lexer, TryParseInput,
  input::{Completeness, InputRef},
};

/// The result the parser [`opt`] builds yields: `Some` on accept, `None` on decline,
/// or the propagated error.
pub type OptOf<'inp, L, Ctx, Lang, O> = Result<Option<O>, ErrorOf<'inp, L, Ctx, Lang>>;

/// Adapts a declining `try_`-parser into one that yields `Option`: an accepted
/// attempt becomes `Some`, a decline becomes `None`.
///
/// The declining sub-parser promises a decline consumes nothing, and `opt` preserves
/// that: on `None` the next token is still in place for the following atom.
///
/// `p` may be any [`TryParseInput`] — the `try_parse`-style atoms and hand-written
/// attempts alike — and the returned closure is itself a
/// [`ParseInput`](crate::ParseInput) (yielding the `Option`) through the blanket impl.
///
/// # Examples
///
/// ```rust
/// # use core::fmt;
/// # use tokora::{FatalContext, InputRef, Lexer, SimpleSpan, Token, error::{UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew}, token::{MissingToken, SeparatedError, UnexpectedToken}}, punct::Comma, span::Span as _, token::PunctuatorToken};
/// # #[derive(Debug)]
/// # struct Error;
/// # impl From<core::convert::Infallible> for Error { fn from(e: core::convert::Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Ident(char), Comma, CloseBrace }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Ident, Comma, CloseBrace }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = core::convert::Infallible;
/// #   fn kind(&self) -> Kind { match self { Tok::Ident(_) => Kind::Ident, Tok::Comma => Kind::Comma, Tok::CloseBrace => Kind::CloseBrace } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn comma() -> Option<Kind> { Some(Kind::Comma) }
/// # }
/// # impl From<Comma<(), (), ()>> for Kind { fn from(_: Comma<(), (), ()>) -> Self { Kind::Comma } }
/// # struct CharLexer<'a> { src: &'a str, pos: usize, tok: SimpleSpan, state: () }
/// # impl<'a> Lexer<'a> for CharLexer<'a> {
/// #   type State = (); type Source = str; type Token = Tok; type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'a str) -> Self { Self { src, pos: 0, tok: SimpleSpan::new(0, 0), state: () } }
/// #   fn with_state(src: &'a str, _: ()) -> Self { Self::new(src) }
/// #   fn check(&self) -> Result<(), core::convert::Infallible> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> Self::State {}
/// #   fn source(&self) -> &'a str { self.src }
/// #   fn span(&self) -> SimpleSpan { self.tok }
/// #   fn slice(&self) -> &'a str { &self.src[self.tok.start()..self.tok.end()] }
/// #   fn lex(&mut self) -> Option<Result<Tok, core::convert::Infallible>> {
/// #     let bytes = self.src.as_bytes();
/// #     while self.pos < bytes.len() && bytes[self.pos] == b' ' { self.pos += 1; }
/// #     if self.pos >= bytes.len() { return None; }
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c { ',' => Tok::Comma, '}' => Tok::CloseBrace, c => Tok::Ident(c) }))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// use tokora::{Parse, Parser, parser::opt};
///
/// // `Comma::try_parse` declines without consuming; `opt` turns the attempt into an
/// // `Option` — `Some` on a comma, `None` on anything else.
/// fn lead_comma<'a>(
///   inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
/// ) -> Result<Option<Comma<SimpleSpan>>, Error> {
///   opt(Comma::try_parse)(inp)
/// }
///
/// let some = Parser::with_parser(lead_comma).parse_str(",").unwrap();
/// assert!(some.is_some());
///
/// let none = Parser::with_parser(lead_comma).parse_str("x").unwrap();
/// assert!(none.is_none()); // declined — the identifier is still unconsumed
/// ```
#[inline]
pub fn opt<'inp, L, Ctx, Lang, P, O, Cmpl>(
  mut p: P,
) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>) -> OptOf<'inp, L, Ctx, Lang, O>
where
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Cmpl: Completeness,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| p.try_parse_input(inp).map(Option::from)
}
