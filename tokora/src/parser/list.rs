use std::vec::Vec;

use generic_arraydeque::typenum::U1;

use crate::{
  Accumulator, ComposableParseContext, Decision, Emitter, ErrorOf, Lexer, ParseInput, Window,
  cache::{Peeked, PeekedTokenExt},
  input::InputRef,
  parser::Action,
  punct::Punctuator,
};

/// The decision the two policy atoms share: continue while the next token exists and
/// satisfies the predicate, stop otherwise — including at end of input, where there is
/// no next token to offer.
///
/// This is the native form of the closure adapter the smear-side originals wrapped
/// around [`Peeked`]: naming the [`Decision`] once lets [`separated1`] and [`list`]
/// hand their caller's bare token predicate straight to the while-drivers.
struct WhileNext<F>(F);

impl<'inp, F, L, E, W, Lang: ?Sized> Decision<'inp, L, E, W, Lang> for WhileNext<F>
where
  F: FnMut(&L::Token) -> bool,
  L: Lexer<'inp>,
  E: Emitter<'inp, L, Lang>,
  W: Window,
{
  #[inline(always)]
  fn decide(&mut self, mut toks: Peeked<'_, 'inp, L, W>, _: &mut E) -> Result<Action, E::Error>
  where
    W: Window,
  {
    Ok(match toks.pop_front() {
      Some(tok) if (self.0)(tok.token()) => Action::Continue,
      _ => Action::Stop,
    })
  }
}

/// The result the parser [`separated1`] builds yields: the collected items, or the
/// propagated error.
#[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
pub type Separated1Of<'inp, L, Ctx, Lang, T> = Result<Vec<T>, ErrorOf<'inp, L, Ctx, Lang>>;

/// The result the parser [`list`] builds yields: the collected items, or the
/// propagated error.
#[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
pub type ListOf<'inp, L, Ctx, Lang, T> = Result<Vec<T>, ErrorOf<'inp, L, Ctx, Lang>>;

/// Builds a parser for one-or-more `item`s separated by the `Sep` punctuator,
/// permitting an optional leading separator, and collects them into a `Vec`.
///
/// A single `Sep` is committed between items and a leading `Sep` is consumed if
/// present, but a trailing one is not accepted — a separator with no item after it is
/// an unexpected-trailing-separator diagnostic. `peek` classifies a non-separator
/// token: the loop takes another item while `peek` returns `true` for the next token
/// and stops otherwise (or at end of input), leaving that token in place. At least one
/// item is required, so an input that yields none is a too-few diagnostic. Both
/// diagnostics go through the context's emitter, so a fail-fast emitter aborts on them
/// while a collecting emitter records them and the parse returns the items gathered so
/// far.
///
/// `item` may be any [`ParseInput`] — a closure or a named parser alike — and the
/// returned closure is itself a `ParseInput` through the blanket impl; `peek` is a
/// predicate, not a parser, and stays a plain closure.
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
/// use tokora::{Parse, Parser, parser::separated1};
///
/// // One-or-more comma-separated identifiers; the optional leading `,` is consumed
/// // and does not count as an item.
/// fn idents<'a>(
///   inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
/// ) -> Result<Vec<char>, Error> {
///   separated1::<Comma, _, _, _, _, _, _>(ident, |tok| matches!(tok, Tok::Ident(_)))(inp)
/// }
///
/// let names = Parser::with_parser(idents).parse_str(", a, b, c").unwrap();
/// assert_eq!(names, vec!['a', 'b', 'c']);
///
/// // A trailing separator is a diagnostic: under the fail-fast context it aborts.
/// assert!(Parser::with_parser(idents).parse_str("a, b,").is_err());
/// ```
#[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
#[inline]
// STAYS COMPLETE-TYPED (0.3.0, R1.7): this constructor's ENGINE is the decision-window
// class — its body drives `separated_while` with a peek predicate, and `SeparatedWhile`
// is Complete-only this release. The returned closure is therefore typed at the default
// (`Complete`) rather than generic over the completeness parameter; it generalizes for
// free once the frontier-window rule lands and the engine relaxes.
pub fn separated1<'inp, Sep, L, Ctx, Lang, P, T, Peek>(
  mut item: P,
  mut peek: Peek,
) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx, Lang>) -> Separated1Of<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Sep: Punctuator<'inp, L, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang>,
  Peek: FnMut(&L::Token) -> bool,
  // The `separated_while` engine surfaces a terminal scanner stop at its separator/decision gates
  // as an end-of-input error; the bundle's `From<UnexpectedEot<L::Offset, Lang>>` absorbs it, so
  // that conversion is no longer restated here.
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang>| {
    item
      .by_ref()
      .separated_while::<Sep, _, U1>(WhileNext(&mut peek))
      .allow_leading()
      .at_least(1)
      .collect()
      .parse_input(inp)
  }
}

/// Builds a parser that applies `item` repeatedly until `until` accepts the next
/// token, collecting the results into a `Vec`.
///
/// Before each item the next token is offered to `until`: the loop stops when `until`
/// returns `true` (or at end of input), leaving that token in place, and otherwise
/// parses another `item`. There is no separator and no lower bound — an input that
/// stops at once yields an empty `Vec` — so the only error surfaced is one an `item`
/// itself propagates.
///
/// `item` may be any [`ParseInput`] — a closure or a named parser alike — and the
/// returned closure is itself a `ParseInput` through the blanket impl; `until` is a
/// predicate, not a parser, and stays a plain closure.
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
/// use tokora::{Parse, Parser, parser::list};
///
/// // Zero-or-more identifiers, stopping at the `}` — which is left in place, so the
/// // caller's next step still sees it.
/// fn fields<'a>(
///   inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
/// ) -> Result<Vec<char>, Error> {
///   let items = list(ident, |tok| matches!(tok, Tok::CloseBrace))(inp)?;
///   let brace = inp.try_expect(|t| matches!(t.data, Tok::CloseBrace))?;
///   assert!(brace.is_some(), "the stop token was left for the caller");
///   Ok(items)
/// }
///
/// let items = Parser::with_parser(fields).parse_str("a b c }").unwrap();
/// assert_eq!(items, vec!['a', 'b', 'c']);
/// ```
#[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
#[inline]
// STAYS COMPLETE-TYPED (0.3.0, R1.7): same engine class as `separated1` — the body
// drives `separated_while`, so the closure stays Complete-typed until that engine relaxes.
pub fn list<'inp, L, Ctx, Lang, P, T, Until>(
  mut item: P,
  mut until: Until,
) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx, Lang>) -> ListOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  P: ParseInput<'inp, L, T, Ctx, Lang>,
  Until: FnMut(&L::Token) -> bool,
  // The `repeated_while` engine surfaces a terminal scanner stop at its decision gate as an
  // end-of-input error; the bundle's `From<UnexpectedEot<L::Offset, Lang>>` absorbs it, so that
  // conversion is no longer restated here.
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang>| {
    item
      .by_ref()
      .repeated_while::<_, U1>(WhileNext(|tok: &L::Token| !until(tok)))
      .collect()
      .parse_input(inp)
  }
}
