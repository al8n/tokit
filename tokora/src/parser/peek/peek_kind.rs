use crate::{ErrorOf, Lexer, ParseCtx, Token, error::UnexpectedEot, input::InputRef};

/// The result [`peek_kind`] returns: the next token's kind, `None` at end of input, or
/// the propagated error.
pub type PeekedKind<'inp, L, Ctx, Lang = ()> =
  Result<Option<<<L as Lexer<'inp>>::Token as Token<'inp>>::Kind>, ErrorOf<'inp, L, Ctx, Lang>>;

/// Reports the kind of the next token without consuming it, or `None` at end of input.
///
/// The dispatch primitive for sum-type composites: a composite peeks one kind and
/// matches it into a committed arm — jump-table style — rather than trying each
/// declining atom in turn. Peeking leaves the token in place, so a subsequent
/// committed atom still parses it. Any lexer error surfacing as the next token is read
/// is propagated.
///
/// # Examples
///
/// ```rust
/// # use core::fmt;
/// # use tokora::{FatalContext, InputRef, Lexer, SimpleSpan, Token, error::{UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew}, token::{MissingToken, SeparatedError, UnexpectedToken}}, punct::Comma, span::Span as _};
/// # #[derive(Debug)]
/// # struct Error;
/// # impl From<core::convert::Infallible> for Error { fn from(e: core::convert::Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for Error { fn from(_: UnexpectedEot<O, Lang>) -> Self { Error } }
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
/// # fn ident<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<char, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Ident(_)))? {
/// #     Some(sp) => match sp.data { Tok::Ident(c) => Ok(c), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Parse, Parser, parser::peek_kind};
///
/// // Peek the dispatch kind without consuming, then let the committed arm parse the
/// // very token that was peeked.
/// fn dispatch<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<char, Error> {
///   match peek_kind(inp)? {
///     Some(Kind::Ident) => ident(inp), // the peeked token is still in place
///     _ => Err(Error),
///   }
/// }
/// let c = Parser::with_parser(dispatch).parse_str("x").unwrap();
/// assert_eq!(c, 'x');
///
/// // End of input peeks as `None` — no token consumed, no error raised.
/// fn at_eof<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<bool, Error> {
///   Ok(peek_kind(inp)?.is_none())
/// }
/// assert!(Parser::with_parser(at_eof).parse_str("").unwrap());
/// ```
#[inline]
pub fn peek_kind<'inp, L, Ctx, Lang>(
  inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
) -> PeekedKind<'inp, L, Ctx, Lang>
where
  L: Lexer<'inp>,
  Ctx: ParseCtx<'inp, L, Lang>,
  Lang: ?Sized,
  ErrorOf<'inp, L, Ctx, Lang>: From<UnexpectedEot<L::Offset, Lang>>,
{
  let mut kind = None;
  // `try_expect_or_stop` with an always-decline predicate: it captures the next token's
  // kind and leaves the token in place (so a committed arm still parses it), yet a terminal
  // scanner stop at the cursor surfaces as an end-of-input error instead of an absent-token
  // `None` — the dispatch must not read a tripped limit as "no token here".
  inp.try_expect_or_stop(|spanned| {
    kind = Some(<L::Token as Token<'inp>>::kind(spanned.data));
    false
  })?;
  Ok(kind)
}
