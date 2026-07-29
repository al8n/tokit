//! Delimited shape atoms: commit an opener, run an inner sub-parser, commit the
//! closer, and wrap the three in a span-carrying [`Delimited`].
//!
//! [`delimited`] is the generic form, taking the [`Delimiter`](crate::delimiter::Delimiter)
//! pair as a type parameter through the [`TypedDelimiter`] capability; [`parens`],
//! [`braces`], [`brackets`], and [`angles`] are the named conveniences fixing that pair
//! to a built-in. Each takes its inner sub-parser as any [`ParseInput`] — a closure, a fn
//! item, or a named implementor — and returns a builder-form parser — a
//! `for<'c> FnMut(&mut InputRef<…>)` closure, itself a `ParseInput` through the blanket
//! impl — so it composes over every lexer, source, and emitter, and drops straight into
//! another combinator with no adapter.
//!
//! A missing closer reports the opener as [`Unclosed`](crate::error::Unclosed) **through the
//! emitter** — the same four-way close-miss law the delimited many-builders
//! (`.delimited::<D>().collect()`) follow: end of input with the opener still open fires
//! `Unclosed` (a fail-fast emitter turns it into `Err`, a recovering emitter records it and
//! yields the construct recovered with a synthesized closer); a wrong token where the closer
//! belongs is the existing unexpected-token (expected-close) diagnostic, **not** `Unclosed`;
//! and a terminal scanner stop surfaces the committed form's end-of-input error, adding no
//! `Unclosed`. This family fires only `Unclosed`, never the
//! [`Unopened`](crate::error::Unopened)/[`Undelimited`](crate::error::Undelimited) half of the
//! recovery vocabulary.
//!
//! Every shape has an **attempt twin** — [`try_delimited`], [`try_parens`], [`try_braces`],
//! [`try_brackets`], [`try_angles`] — that declines (`Ok(None)`, zero consumption) **iff
//! the opener is definitely absent**: the next valid token is not the opener (it stays
//! unconsumed), or the input has genuinely ended. A **terminal stop** at the would-be
//! opener — a resource-limit trip, fresh or already latched — is *neither*: it is not
//! evidence the opener is absent, so the attempt does not decline; it fails with the same
//! end-of-input error the committed form raises there.
//! The moment the opener is consumed the parse is committed, and every later error
//! propagates exactly as the committed form's. The attempt boundary is deliberately the
//! opener alone, never the whole shape — see [`try_delimited`] for why.

use crate::{
  ComposableParseContext, Emitter, ErrorOf, Lexer, ParseInput, Token,
  delimiter::{DelimiterKind, TypedDelimiter},
  emitter::UnclosedEmitter,
  error::{Unclosed, UnexpectedEot, token::UnexpectedTokenOf},
  input::{CloseStatus, Cursor, InputRef, SurfaceIncomplete},
  punct::{
    Angle, Brace, Bracket, CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace,
    OpenBracket, OpenParen, Paren,
  },
  span::{Span as _, Spanned},
  token::{PunctuatorToken, PunctuatorTokenExt, SpannedPunctuatorToken},
  try_parse_input::ParseAttempt,
  utils::{CowStr, Delimited},
};

/// The result the parser [`delimited`] builds yields: `inner`'s output wrapped in a
/// `D`-delimited [`Delimited`] spanning the whole construct, or the propagated error.
pub type DelimitedOf<'inp, D, L, Ctx, Lang, T> = Result<
  Delimited<
    <D as TypedDelimiter<'inp, L, Lang>>::OpenValue,
    <D as TypedDelimiter<'inp, L, Lang>>::CloseValue,
    T,
    <L as Lexer<'inp>>::Span,
  >,
  ErrorOf<'inp, L, Ctx, Lang>,
>;

/// Commits the `D` opener, runs `inner`, commits the `D` closer, and returns the three as
/// a [`Delimited`] whose span covers the whole construct.
///
/// `D` is a [`Delimiter`](crate::delimiter::Delimiter) pair passed as the first type
/// parameter (`delimited::<Paren, …>(inner)`, mirroring the many-builder's
/// [`.delimited::<D>()`](crate::parser::Separated::delimited) and the atom
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " [`separated1::<Sep, …>`](crate::parser::separated1)); its [`TypedDelimiter`] impl"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " `separated1::<Sep, …>`); its [`TypedDelimiter`] impl"
)]
/// supplies the span-carrying punctuator values the result stores. For the built-in pairs,
/// `delimited::<Paren, …>(inner)` is equivalent to [`parens(inner)`](parens) — and likewise
/// for [`braces`]/[`brackets`]/[`angles`] — for any vocabulary whose two capability
/// declarations (`PunctuatorToken::open_paren` and `Kind: From<OpenParen<(), (), ()>>`)
/// agree, as all the built-in fixtures do.
///
/// `inner` runs between the committed delimiters and its output becomes the [`Delimited`]
/// data. A missing closer reports the opener as [`Unclosed`] through the emitter: a fail-fast
/// emitter turns it into `Err`, a recovering emitter records it and yields the group recovered
/// with a synthesized closer. A wrong token where the closer belongs stays the unexpected-token
/// (expected-close) diagnostic instead.
///
/// `inner` may be any [`ParseInput`](crate::ParseInput) — a closure or a named parser alike
/// — and the returned closure is itself a `ParseInput` through the blanket impl, so shapes
/// nest and compose without adapters.
///
/// # Examples
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug, PartialEq)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Digit(u32), LParen, RParen, LBracket, RBracket, LBrace, RBrace, LAngle, RAngle }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Digit, LParen, RParen, LBracket, RBracket, LBrace, RBrace, LAngle, RAngle }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> Kind { match self {
/// #     Tok::Digit(_) => Kind::Digit,
/// #     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen,
/// #     Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket,
/// #     Tok::LBrace => Kind::LBrace, Tok::RBrace => Kind::RBrace,
/// #     Tok::LAngle => Kind::LAngle, Tok::RAngle => Kind::RAngle } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
/// #   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
/// #   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
/// #   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
/// #   fn open_brace() -> Option<Kind> { Some(Kind::LBrace) }
/// #   fn close_brace() -> Option<Kind> { Some(Kind::RBrace) }
/// #   fn open_angle() -> Option<Kind> { Some(Kind::LAngle) }
/// #   fn close_angle() -> Option<Kind> { Some(Kind::RAngle) }
/// # }
/// # impl From<OpenParen<(), (), ()>> for Kind { fn from(_: OpenParen<(), (), ()>) -> Self { Kind::LParen } }
/// # impl From<CloseParen<(), (), ()>> for Kind { fn from(_: CloseParen<(), (), ()>) -> Self { Kind::RParen } }
/// # impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
/// # impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
/// # impl From<OpenBrace<(), (), ()>> for Kind { fn from(_: OpenBrace<(), (), ()>) -> Self { Kind::LBrace } }
/// # impl From<CloseBrace<(), (), ()>> for Kind { fn from(_: CloseBrace<(), (), ()>) -> Self { Kind::RBrace } }
/// # impl From<OpenAngle<(), (), ()>> for Kind { fn from(_: OpenAngle<(), (), ()>) -> Self { Kind::LAngle } }
/// # impl From<CloseAngle<(), (), ()>> for Kind { fn from(_: CloseAngle<(), (), ()>) -> Self { Kind::RAngle } }
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
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c {
/// #       '0'..='9' => Tok::Digit(c as u32 - '0' as u32),
/// #       '(' => Tok::LParen, ')' => Tok::RParen, '[' => Tok::LBracket, ']' => Tok::RBracket,
/// #       '{' => Tok::LBrace, '}' => Tok::RBrace, '<' => Tok::LAngle, '>' => Tok::RAngle,
/// #       _ => Tok::Digit(0),
/// #     }))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Digit(_)))? {
/// #     Some(sp) => match sp.data { Tok::Digit(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Parse, Parser, punct::Paren, parser::delimited};
///
/// // A parenthesized digit through the generic form, fixing the pair to `Paren`.
/// fn paren_digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
///     delimited::<Paren, _, _, _, _, _, _>(digit)(inp).map(|d| *d.data())
/// }
///
/// assert_eq!(Parser::with_parser(paren_digit).parse_str("(1)").unwrap(), 1);
/// // A missing closer is a hard error, not a fabricated delimiter.
/// assert!(Parser::with_parser(paren_digit).parse_str("(1").is_err());
/// ```
#[inline]
pub fn delimited<'inp, D, L, Ctx, Lang, P, T, Cmpl>(
  mut inner: P,
) -> impl for<'c> FnMut(
  &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
) -> DelimitedOf<'inp, D, L, Ctx, Lang, T>
where
  D: TypedDelimiter<'inp, L, Lang>,
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    let cursor = inp.cursor().clone();
    let open_span = match inp.next_or_stop()? {
      Some(sp) if D::is_open(&sp.data().kind()) => sp.into_span(),
      Some(sp) => return Err(D::unexpected_open_token(sp).into()),
      None => return Err(UnexpectedEot::eot_of(inp.span().end()).into()),
    };
    finish_delimited::<D, _, _, _, _, _, _>(inp, &cursor, open_span, &mut inner)
  }
}

/// The two halves of a pair's identity an [`Unclosed`] carries: the machine kind a conversion
/// routes on, and the display name it renders.
///
/// They cross as one argument because they always travel together and always come from the
/// same place. For the four built-in shapes that is load-bearing rather than tidy: the
/// associated constants below are the only spelling those call sites use, so a shape parser
/// cannot pass one pair's kind next to another pair's name.
struct PairIdentity {
  kind: DelimiterKind,
  name: CowStr,
}

impl PairIdentity {
  const PAREN: Self = Self::new(DelimiterKind::Paren, CowStr::from_static("()"));
  const BRACE: Self = Self::new(DelimiterKind::Brace, CowStr::from_static("{}"));
  const BRACKET: Self = Self::new(DelimiterKind::Bracket, CowStr::from_static("[]"));
  const ANGLE: Self = Self::new(DelimiterKind::Angle, CowStr::from_static("<>"));

  const fn new(kind: DelimiterKind, name: CowStr) -> Self {
    Self { kind, name }
  }
}

/// The one close-miss law every delimited shape shares — the shape-family twin of the
/// delimited many-builder's [`probe_close`](InputRef::probe_close) mapping (see
/// `parser::many::delim`). With the inner sub-parser already run, classify the close
/// position four ways and either commit the closer or diagnose the miss **through the
/// emitter** and recover:
///
/// - [`Close`](CloseStatus::Close): the closer is at hand — commit the probed token by value
///   with [`commit_probed`](InputRef::commit_probed), re-scanning nothing in any cache
///   capacity, and return its materialized value;
/// - [`WrongToken`](CloseStatus::WrongToken): a non-closer sits where the closer belongs —
///   `expect_close` turns it into the expected-close [`UnexpectedToken`](crate::error::token::UnexpectedToken), emitted through the
///   emitter (a fail-fast emitter turns it into `Err`, a recovering one records it), then the
///   shape recovers with a closer synthesized at the current cursor (the committed frontier /
///   the wrong token's start, per cache);
/// - [`Eof`](CloseStatus::Eof): end of input with the opener still open — the one and only
///   [`Unclosed`] path: emit it anchored at `open_span`, then recover with a synthesized
///   closer;
/// - [`Tripped`](CloseStatus::Tripped): a terminal scanner stop whose own diagnostic already
///   explains the halt — surface the committed form's end-of-input error, adding no
///   `Unclosed`.
///
/// Returns the close value together with the whole-construct span (from `cursor`, captured
/// before the opener, to the close position). The recovering paths synthesize the close as a
/// zero-width span at `inp.cursor()` and span the construct via `inp.span_since(cursor)`,
/// exactly as the delimited many-builder does; both end at the live cursor, so
/// `close.span().end() == <shape>.span().end() == inp.cursor()` and the recovered shape never
/// outruns the cursor (an enclosing parent's span always contains it). Cache capacity only
/// moves the cursor within the trailing-trivia gap — the committed frontier under the blackhole
/// `()`, the wrong token's start under a retaining cache — matching the many-builder in every
/// capacity.
#[inline]
fn commit_delim_close<'inp, 'c, D, L, Ctx, Lang, Cmpl, CV>(
  inp: &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
  cursor: &Cursor<'inp, 'c, L>,
  open_span: &L::Span,
  pair: PairIdentity,
  is_close: impl Fn(&L::Token) -> bool,
  expect_close: impl FnOnce(
    Spanned<L::Token, L::Span>,
  ) -> Result<Spanned<L::Token, L::Span>, UnexpectedTokenOf<'inp, L, Lang>>,
  make_close: impl Fn(L::Span) -> CV,
) -> Result<(CV, L::Span), ErrorOf<'inp, L, Ctx, Lang>>
where
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  match inp.probe_close(|t| is_close(t.data))? {
    // The closer is at hand: commit the probed token by value via `commit_probed` — no
    // re-scan, and cache-independent (a blackhole `()` would drop a pushed-back closer) — and
    // span the construct to its committed end.
    CloseStatus::Close(payload) => {
      let closer = inp.commit_probed(payload);
      Ok((make_close(closer.into_span()), inp.span_since(cursor)))
    }
    // A non-closer where the closer belongs: the existing expected-close diagnostic (unchanged
    // in kind), routed through the emitter so a recovering emitter records it. Then recover at
    // the LIVE cursor — the committed frontier under the blackhole `()`, the wrong token's start
    // under a retaining cache — exactly as the delimited many-builder's `span_since(cursor)`. The
    // recovered span never outruns the cursor, so an enclosing parent contains the shape and
    // `close.end() == shape.end() == inp.cursor()`.
    CloseStatus::WrongToken(tok) => match expect_close(tok) {
      // Unreachable — the probe reported `WrongToken` — but commit it if it is the closer.
      Ok(closer) => Ok((make_close(closer.into_span()), inp.span_since(cursor))),
      Err(err) => {
        inp.emitter().emit_unexpected_token(err)?;
        Ok((
          make_close(inp.span_since(inp.cursor())),
          inp.span_since(cursor),
        ))
      }
    },
    // End of input with the opener still open: the one and only `Unclosed` path. Nothing is
    // cached at EOF (no token), so the live cursor already is the committed frontier.
    CloseStatus::Eof => {
      inp
        .emitter()
        .emit_unclosed(Unclosed::<D, L::Span, Lang>::of(
          open_span.clone(),
          pair.kind,
          pair.name,
        ))?;
      Ok((
        make_close(inp.span_since(inp.cursor())),
        inp.span_since(cursor),
      ))
    }
    // A terminal scanner stop already carries its own diagnostic: surface the committed form's
    // end-of-input error, marked terminal so recovery re-raises it, and add no `Unclosed`.
    CloseStatus::Tripped => Err(
      UnexpectedEot::eot_of(inp.span().end())
        .into_terminal()
        .into(),
    ),
  }
}

/// The shared post-open body of [`delimited`] and [`try_delimited`]: from here the parse
/// is committed — runs `inner`, commits the `D` closer (or reports the close-miss through
/// the emitter and recovers — see [`commit_delim_close`]), and builds the [`Delimited`]
/// whose span runs from `cursor` (captured before the opener) to the closer.
#[inline]
fn finish_delimited<'inp, 'c, D, L, Ctx, Lang, P, T, Cmpl>(
  inp: &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
  cursor: &Cursor<'inp, 'c, L>,
  open_span: L::Span,
  inner: &mut P,
) -> DelimitedOf<'inp, D, L, Ctx, Lang, T>
where
  D: TypedDelimiter<'inp, L, Lang>,
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  let data = inner.parse_input(inp)?;
  let (close, span) = commit_delim_close::<D, _, _, _, _, _>(
    inp,
    cursor,
    &open_span,
    PairIdentity::new(D::KIND, D::name()),
    |t| D::is_close(&t.kind()),
    |tok| Err(D::unexpected_close_token(tok)),
    D::close_value,
  )?;
  Ok(Delimited::new(D::open_value(open_span), close, data, span))
}

/// The result the parser [`try_delimited`] builds yields: `Some` of the `D`-delimited
/// construct on accept, `None` on decline, or the propagated error.
pub type TryDelimitedOf<'inp, D, L, Ctx, Lang, T> = Result<
  Option<
    Delimited<
      <D as TypedDelimiter<'inp, L, Lang>>::OpenValue,
      <D as TypedDelimiter<'inp, L, Lang>>::CloseValue,
      T,
      <L as Lexer<'inp>>::Span,
    >,
  >,
  ErrorOf<'inp, L, Ctx, Lang>,
>;

/// The attempt twin of [`delimited`]: tries to commit the `D` opener and, if it is next,
/// parses the rest of the construct exactly as [`delimited`] does.
///
/// Declines — `Ok(None)`, zero consumption — **iff the opener is definitely absent**: the
/// next valid token is not the `D` opener (it stays unconsumed), or the input has
/// genuinely ended. A **terminal stop** at the would-be opener — a resource-limit trip,
/// fresh or already latched — is *neither*: it is not evidence the opener is absent, so
/// the attempt does not decline. It fails instead with the same end-of-input error the
/// committed form raises there (the trip's own diagnostic having already reached the
/// emitter), so an optional construct can never be silently skipped because the lexer was
/// stopped rather than satisfied. The moment the opener is consumed the parse is
/// **committed**: `inner`'s errors and the missing/wrong-closer diagnostics behave exactly
/// as [`delimited`]'s do — an unterminated group reports the opener as [`Unclosed`] through
/// the emitter (a fail-fast `Err`, a recovering record), never a silent decline.
///
/// This is deliberately not `opt(delimited(inner))`: a whole-shape attempt would swallow
/// an unclosed-delimiter error into a decline — for a generics-like grammar, `Ident<` at
/// end of input must error as unclosed rather than silently decline and reparse. The
/// attempt boundary is the opener alone.
///
/// `inner` may be any [`ParseInput`](crate::ParseInput); the returned closure is itself
/// a `ParseInput` (yielding the `Option`) through the blanket impl.
///
/// # Examples
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug, PartialEq)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Ident(char), LAngle, RAngle }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Ident, LAngle, RAngle }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> Kind { match self { Tok::Ident(_) => Kind::Ident, Tok::LAngle => Kind::LAngle, Tok::RAngle => Kind::RAngle } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_angle() -> Option<Kind> { Some(Kind::LAngle) }
/// #   fn close_angle() -> Option<Kind> { Some(Kind::RAngle) }
/// # }
/// # impl From<OpenAngle<(), (), ()>> for Kind { fn from(_: OpenAngle<(), (), ()>) -> Self { Kind::LAngle } }
/// # impl From<CloseAngle<(), (), ()>> for Kind { fn from(_: CloseAngle<(), (), ()>) -> Self { Kind::RAngle } }
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
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c { '<' => Tok::LAngle, '>' => Tok::RAngle, c => Tok::Ident(c) }))
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
/// use tokora::{Parse, Parser, punct::Angle, parser::try_delimited};
///
/// // A name with optional generics — `x<t>` or plain `x` — the shape where the attempt
/// // boundary matters.
/// fn generic_name<'a>(
///   inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
/// ) -> Result<(char, Option<char>), Error> {
///     let name = ident(inp)?;
///     let args = try_delimited::<Angle, _, _, _, _, _, _>(ident)(inp)?;
///     Ok((name, args.map(|d| *d.data())))
/// }
///
/// assert_eq!(Parser::with_parser(generic_name).parse_str("x<t>").unwrap(), ('x', Some('t')));
/// // No `<` follows (here: end of input) — the attempt declines and nothing is consumed.
/// assert_eq!(Parser::with_parser(generic_name).parse_str("x").unwrap(), ('x', None));
/// // Once `<` is consumed the parse is committed: unclosed generics ERROR, they do not
/// // silently decline (which `opt(delimited(…))` would wrongly do).
/// assert!(Parser::with_parser(generic_name).parse_str("x<t").is_err());
/// ```
#[inline]
pub fn try_delimited<'inp, D, L, Ctx, Lang, P, T, Cmpl>(
  mut inner: P,
) -> impl for<'c> FnMut(
  &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
) -> TryDelimitedOf<'inp, D, L, Ctx, Lang, T>
where
  D: TypedDelimiter<'inp, L, Lang>,
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    let cursor = inp.cursor().clone();
    let open_span = match inp.try_expect_or_stop(|tok| D::is_open(&tok.data.kind()))? {
      Some(sp) => sp.into_span(),
      None => return Ok(None),
    };
    finish_delimited::<D, _, _, _, _, _, _>(inp, &cursor, open_span, &mut inner).map(Some)
  }
}

/// The result [`parens`] returns: `inner`'s output wrapped in a paren-delimited
/// [`Delimited`] spanning the whole `( … )`, or the propagated error.
pub type ParensOf<'inp, L, Ctx, Lang, T> = Result<
  Delimited<
    OpenParen<<L as Lexer<'inp>>::Span, (), Lang>,
    CloseParen<<L as Lexer<'inp>>::Span, (), Lang>,
    T,
    <L as Lexer<'inp>>::Span,
  >,
  ErrorOf<'inp, L, Ctx, Lang>,
>;

/// Commits the `(` opener, runs `inner`, commits the `)` closer, and returns the
/// three as a [`Delimited`] whose span covers the whole `( … )`.
///
/// `inner` runs between the committed delimiters and its output becomes the
/// [`Delimited`] data. A missing closer reports the opener as [`Unclosed`] through the
/// emitter — a fail-fast emitter turns it into `Err`, a recovering emitter records it and
/// yields the `( …` group recovered with a synthesized `)`. A wrong token where `)` belongs
/// stays the unexpected-token (expected-close) diagnostic.
///
/// `inner` may be any [`ParseInput`](crate::ParseInput) — a closure or a named parser alike
/// — and the returned closure is itself a `ParseInput` through the blanket impl, so shapes
/// nest and compose without adapters.
///
/// # Examples
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug, PartialEq)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Digit(u32), LParen, RParen }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Digit, LParen, RParen }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> Kind { match self { Tok::Digit(_) => Kind::Digit, Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
/// #   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
/// # }
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
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c { '0'..='9' => Tok::Digit(c as u32 - '0' as u32), '(' => Tok::LParen, ')' => Tok::RParen, _ => Tok::Digit(0) }))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Digit(_)))? {
/// #     Some(sp) => match sp.data { Tok::Digit(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Parse, Parser, parser::parens};
///
/// fn wrapped<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
///     parens(digit)(inp).map(|d| *d.data())
/// }
///
/// assert_eq!(Parser::with_parser(wrapped).parse_str("(1)").unwrap(), 1);
/// assert!(Parser::with_parser(wrapped).parse_str("(1").is_err());
/// ```
#[inline]
pub fn parens<'inp, L, Ctx, Lang, P, T, Cmpl>(
  mut inner: P,
) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>) -> ParensOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    let cursor = inp.cursor().clone();
    let open = OpenParen::parse(inp)?;
    finish_parens(inp, &cursor, open, &mut inner)
  }
}

/// The shared post-open body of [`parens`] and [`try_parens`]: from here the parse is
/// committed — runs `inner`, commits the `)` closer, and builds the [`Delimited`] whose
/// span runs from `cursor` (captured before the opener) to the closer.
#[inline]
fn finish_parens<'inp, 'c, L, Ctx, Lang, P, T, Cmpl>(
  inp: &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
  cursor: &Cursor<'inp, 'c, L>,
  open: OpenParen<L::Span, (), Lang>,
  inner: &mut P,
) -> ParensOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  let data = inner.parse_input(inp)?;
  let (close, span) = commit_delim_close::<Paren<(), (), Lang>, _, _, _, _, _>(
    inp,
    cursor,
    open.span(),
    PairIdentity::PAREN,
    |t| t.is_close_paren(),
    |tok| SpannedPunctuatorToken::<'inp, L, Lang>::expect_close_paren(tok),
    |s| CloseParen::new(s).change_language::<Lang>(),
  )?;
  Ok(Delimited::new(open, close, data, span))
}

/// The result the parser [`try_parens`] builds yields: `Some` of the paren-delimited
/// construct on accept, `None` on decline, or the propagated error.
pub type TryParensOf<'inp, L, Ctx, Lang, T> = Result<
  Option<
    Delimited<
      OpenParen<<L as Lexer<'inp>>::Span, (), Lang>,
      CloseParen<<L as Lexer<'inp>>::Span, (), Lang>,
      T,
      <L as Lexer<'inp>>::Span,
    >,
  >,
  ErrorOf<'inp, L, Ctx, Lang>,
>;

/// The attempt twin of [`parens`]: tries to commit the `(` opener and, if it is next,
/// parses the rest of the group exactly as [`parens`] does.
///
/// Declines — `Ok(None)`, zero consumption — **iff the opener is definitely absent**: the
/// next valid token is not `(` (it stays unconsumed), or the input has genuinely ended. A
/// **terminal stop** at the would-be opener — a resource-limit trip, fresh or already
/// latched — is *neither*: it is not evidence the opener is absent, so the attempt does
/// not decline. It fails instead with the same end-of-input error the committed form
/// raises there (the trip's own diagnostic having already reached the emitter), so an
/// optional construct can never be silently skipped because the lexer was stopped rather
/// than satisfied. The moment the `(` opener is consumed the parse is **committed**:
/// `inner`'s errors and the missing/wrong `)` closer's diagnostics behave exactly as
/// [`parens`]' do — an unterminated `( …` reports the opener as [`Unclosed`] through the
/// emitter, never a silent decline (see [`try_delimited`] for why the attempt boundary is the
/// opener alone).
///
/// `inner` may be any [`ParseInput`](crate::ParseInput); the returned closure is itself
/// a `ParseInput` (yielding the `Option`) through the blanket impl.
///
/// # Examples
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug, PartialEq)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Digit(u32), LParen, RParen }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Digit, LParen, RParen }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> Kind { match self { Tok::Digit(_) => Kind::Digit, Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
/// #   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
/// # }
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
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c { '0'..='9' => Tok::Digit(c as u32 - '0' as u32), '(' => Tok::LParen, ')' => Tok::RParen, _ => Tok::Digit(0) }))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Digit(_)))? {
/// #     Some(sp) => match sp.data { Tok::Digit(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Parse, Parser, parser::try_parens};
///
/// fn attempt<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Option<u32>, Error> {
///     try_parens(digit)(inp).map(|d| d.map(|d| *d.data()))
/// }
///
/// assert_eq!(Parser::with_parser(attempt).parse_str("(1)").unwrap(), Some(1));
/// // No `(` opener: the attempt declines and the digit is left unconsumed.
/// assert_eq!(Parser::with_parser(attempt).parse_str("1").unwrap(), None);
/// // Opener consumed: committed — an unterminated group errors, it does not decline.
/// assert!(Parser::with_parser(attempt).parse_str("(1").is_err());
/// ```
#[inline]
pub fn try_parens<'inp, L, Ctx, Lang, P, T, Cmpl>(
  mut inner: P,
) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>) -> TryParensOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    let cursor = inp.cursor().clone();
    match OpenParen::try_parse(inp)? {
      ParseAttempt::Accept(open) => finish_parens(inp, &cursor, open, &mut inner).map(Some),
      ParseAttempt::Decline => Ok(None),
    }
  }
}

/// The result [`braces`] returns: `inner`'s output wrapped in a brace-delimited
/// [`Delimited`] spanning the whole `{ … }`, or the propagated error.
pub type BracesOf<'inp, L, Ctx, Lang, T> = Result<
  Delimited<
    OpenBrace<<L as Lexer<'inp>>::Span, (), Lang>,
    CloseBrace<<L as Lexer<'inp>>::Span, (), Lang>,
    T,
    <L as Lexer<'inp>>::Span,
  >,
  ErrorOf<'inp, L, Ctx, Lang>,
>;

/// Commits the `{` opener, runs `inner`, commits the `}` closer, and returns the
/// three as a [`Delimited`] whose span covers the whole `{ … }`.
///
/// `inner` runs between the committed delimiters and its output becomes the
/// [`Delimited`] data. A missing closer reports the opener as [`Unclosed`] through the
/// emitter — a fail-fast emitter turns it into `Err`, a recovering emitter records it and
/// yields the `{ …` group recovered with a synthesized `}`. A wrong token where `}` belongs
/// stays the unexpected-token (expected-close) diagnostic.
///
/// `inner` may be any [`ParseInput`](crate::ParseInput) — a closure or a named parser alike
/// — and the returned closure is itself a `ParseInput` through the blanket impl, so shapes
/// nest and compose without adapters.
///
/// # Examples
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug, PartialEq)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Digit(u32), LBrace, RBrace }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Digit, LBrace, RBrace }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> Kind { match self { Tok::Digit(_) => Kind::Digit, Tok::LBrace => Kind::LBrace, Tok::RBrace => Kind::RBrace } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_brace() -> Option<Kind> { Some(Kind::LBrace) }
/// #   fn close_brace() -> Option<Kind> { Some(Kind::RBrace) }
/// # }
/// # impl From<OpenBrace<(), (), ()>> for Kind { fn from(_: OpenBrace<(), (), ()>) -> Self { Kind::LBrace } }
/// # impl From<CloseBrace<(), (), ()>> for Kind { fn from(_: CloseBrace<(), (), ()>) -> Self { Kind::RBrace } }
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
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c { '0'..='9' => Tok::Digit(c as u32 - '0' as u32), '{' => Tok::LBrace, '}' => Tok::RBrace, _ => Tok::Digit(0) }))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Digit(_)))? {
/// #     Some(sp) => match sp.data { Tok::Digit(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Parse, Parser, parser::braces};
///
/// fn wrapped<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
///     braces(digit)(inp).map(|d| *d.data())
/// }
///
/// assert_eq!(Parser::with_parser(wrapped).parse_str("{1}").unwrap(), 1);
/// assert!(Parser::with_parser(wrapped).parse_str("{1").is_err());
/// ```
#[inline]
pub fn braces<'inp, L, Ctx, Lang, P, T, Cmpl>(
  mut inner: P,
) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>) -> BracesOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    let cursor = inp.cursor().clone();
    let open = OpenBrace::parse(inp)?;
    finish_braces(inp, &cursor, open, &mut inner)
  }
}

/// The shared post-open body of [`braces`] and [`try_braces`]: from here the parse is
/// committed — runs `inner`, commits the `}` closer, and builds the [`Delimited`] whose
/// span runs from `cursor` (captured before the opener) to the closer.
#[inline]
fn finish_braces<'inp, 'c, L, Ctx, Lang, P, T, Cmpl>(
  inp: &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
  cursor: &Cursor<'inp, 'c, L>,
  open: OpenBrace<L::Span, (), Lang>,
  inner: &mut P,
) -> BracesOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  let data = inner.parse_input(inp)?;
  let (close, span) = commit_delim_close::<Brace<(), (), Lang>, _, _, _, _, _>(
    inp,
    cursor,
    open.span(),
    PairIdentity::BRACE,
    |t| t.is_close_brace(),
    |tok| SpannedPunctuatorToken::<'inp, L, Lang>::expect_close_brace(tok),
    |s| CloseBrace::new(s).change_language::<Lang>(),
  )?;
  Ok(Delimited::new(open, close, data, span))
}

/// The result the parser [`try_braces`] builds yields: `Some` of the brace-delimited
/// construct on accept, `None` on decline, or the propagated error.
pub type TryBracesOf<'inp, L, Ctx, Lang, T> = Result<
  Option<
    Delimited<
      OpenBrace<<L as Lexer<'inp>>::Span, (), Lang>,
      CloseBrace<<L as Lexer<'inp>>::Span, (), Lang>,
      T,
      <L as Lexer<'inp>>::Span,
    >,
  >,
  ErrorOf<'inp, L, Ctx, Lang>,
>;

/// The attempt twin of [`braces`]: tries to commit the `{` opener and, if it is next,
/// parses the rest of the group exactly as [`braces`] does.
///
/// Declines — `Ok(None)`, zero consumption — **iff the opener is definitely absent**: the
/// next valid token is not `{` (it stays unconsumed), or the input has genuinely ended. A
/// **terminal stop** at the would-be opener — a resource-limit trip, fresh or already
/// latched — is *neither*: it is not evidence the opener is absent, so the attempt does
/// not decline. It fails instead with the same end-of-input error the committed form
/// raises there (the trip's own diagnostic having already reached the emitter), so an
/// optional construct can never be silently skipped because the lexer was stopped rather
/// than satisfied. The moment the `{` opener is consumed the parse is **committed**:
/// `inner`'s errors and the missing/wrong `}` closer's diagnostics behave exactly as
/// [`braces`]' do — an unterminated `{ …` reports the opener as [`Unclosed`] through the
/// emitter, never a silent decline (see [`try_delimited`] for why the attempt boundary is the
/// opener alone).
///
/// `inner` may be any [`ParseInput`](crate::ParseInput); the returned closure is itself
/// a `ParseInput` (yielding the `Option`) through the blanket impl.
///
/// # Examples
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug, PartialEq)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Digit(u32), LBrace, RBrace }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Digit, LBrace, RBrace }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> Kind { match self { Tok::Digit(_) => Kind::Digit, Tok::LBrace => Kind::LBrace, Tok::RBrace => Kind::RBrace } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_brace() -> Option<Kind> { Some(Kind::LBrace) }
/// #   fn close_brace() -> Option<Kind> { Some(Kind::RBrace) }
/// # }
/// # impl From<OpenBrace<(), (), ()>> for Kind { fn from(_: OpenBrace<(), (), ()>) -> Self { Kind::LBrace } }
/// # impl From<CloseBrace<(), (), ()>> for Kind { fn from(_: CloseBrace<(), (), ()>) -> Self { Kind::RBrace } }
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
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c { '0'..='9' => Tok::Digit(c as u32 - '0' as u32), '{' => Tok::LBrace, '}' => Tok::RBrace, _ => Tok::Digit(0) }))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Digit(_)))? {
/// #     Some(sp) => match sp.data { Tok::Digit(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Parse, Parser, parser::try_braces};
///
/// fn attempt<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Option<u32>, Error> {
///     try_braces(digit)(inp).map(|d| d.map(|d| *d.data()))
/// }
///
/// assert_eq!(Parser::with_parser(attempt).parse_str("{1}").unwrap(), Some(1));
/// // No `{` opener: the attempt declines and the digit is left unconsumed.
/// assert_eq!(Parser::with_parser(attempt).parse_str("1").unwrap(), None);
/// // Opener consumed: committed — an unterminated group errors, it does not decline.
/// assert!(Parser::with_parser(attempt).parse_str("{1").is_err());
/// ```
#[inline]
pub fn try_braces<'inp, L, Ctx, Lang, P, T, Cmpl>(
  mut inner: P,
) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>) -> TryBracesOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    let cursor = inp.cursor().clone();
    match OpenBrace::try_parse(inp)? {
      ParseAttempt::Accept(open) => finish_braces(inp, &cursor, open, &mut inner).map(Some),
      ParseAttempt::Decline => Ok(None),
    }
  }
}

/// The result [`brackets`] returns: `inner`'s output wrapped in a
/// bracket-delimited [`Delimited`] spanning the whole `[ … ]`, or the propagated
/// error.
pub type BracketsOf<'inp, L, Ctx, Lang, T> = Result<
  Delimited<
    OpenBracket<<L as Lexer<'inp>>::Span, (), Lang>,
    CloseBracket<<L as Lexer<'inp>>::Span, (), Lang>,
    T,
    <L as Lexer<'inp>>::Span,
  >,
  ErrorOf<'inp, L, Ctx, Lang>,
>;

/// Commits the `[` opener, runs `inner`, commits the `]` closer, and returns the
/// three as a [`Delimited`] whose span covers the whole `[ … ]`.
///
/// `inner` runs between the committed delimiters and its output becomes the
/// [`Delimited`] data. A missing closer reports the opener as [`Unclosed`] through the
/// emitter — a fail-fast emitter turns it into `Err`, a recovering emitter records it and
/// yields the `[ …` group recovered with a synthesized `]`. A wrong token where `]` belongs
/// stays the unexpected-token (expected-close) diagnostic.
///
/// `inner` may be any [`ParseInput`](crate::ParseInput) — a closure or a named parser alike
/// — and the returned closure is itself a `ParseInput` through the blanket impl, so shapes
/// nest and compose without adapters.
///
/// # Examples
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug, PartialEq)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Digit(u32), LBracket, RBracket }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Digit, LBracket, RBracket }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> Kind { match self { Tok::Digit(_) => Kind::Digit, Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
/// #   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
/// # }
/// # impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
/// # impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
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
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c { '0'..='9' => Tok::Digit(c as u32 - '0' as u32), '[' => Tok::LBracket, ']' => Tok::RBracket, _ => Tok::Digit(0) }))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Digit(_)))? {
/// #     Some(sp) => match sp.data { Tok::Digit(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Parse, Parser, parser::brackets};
///
/// fn wrapped<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
///     brackets(digit)(inp).map(|d| *d.data())
/// }
///
/// assert_eq!(Parser::with_parser(wrapped).parse_str("[1]").unwrap(), 1);
/// assert!(Parser::with_parser(wrapped).parse_str("[1").is_err());
/// ```
#[inline]
pub fn brackets<'inp, L, Ctx, Lang, P, T, Cmpl>(
  mut inner: P,
) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>) -> BracketsOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    let cursor = inp.cursor().clone();
    let open = OpenBracket::parse(inp)?;
    finish_brackets(inp, &cursor, open, &mut inner)
  }
}

/// The shared post-open body of [`brackets`] and [`try_brackets`]: from here the parse is
/// committed — runs `inner`, commits the `]` closer, and builds the [`Delimited`] whose
/// span runs from `cursor` (captured before the opener) to the closer.
#[inline]
fn finish_brackets<'inp, 'c, L, Ctx, Lang, P, T, Cmpl>(
  inp: &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
  cursor: &Cursor<'inp, 'c, L>,
  open: OpenBracket<L::Span, (), Lang>,
  inner: &mut P,
) -> BracketsOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  let data = inner.parse_input(inp)?;
  let (close, span) = commit_delim_close::<Bracket<(), (), Lang>, _, _, _, _, _>(
    inp,
    cursor,
    open.span(),
    PairIdentity::BRACKET,
    |t| t.is_close_bracket(),
    |tok| SpannedPunctuatorToken::<'inp, L, Lang>::expect_close_bracket(tok),
    |s| CloseBracket::new(s).change_language::<Lang>(),
  )?;
  Ok(Delimited::new(open, close, data, span))
}

/// The result the parser [`try_brackets`] builds yields: `Some` of the bracket-delimited
/// construct on accept, `None` on decline, or the propagated error.
pub type TryBracketsOf<'inp, L, Ctx, Lang, T> = Result<
  Option<
    Delimited<
      OpenBracket<<L as Lexer<'inp>>::Span, (), Lang>,
      CloseBracket<<L as Lexer<'inp>>::Span, (), Lang>,
      T,
      <L as Lexer<'inp>>::Span,
    >,
  >,
  ErrorOf<'inp, L, Ctx, Lang>,
>;

/// The attempt twin of [`brackets`]: tries to commit the `[` opener and, if it is next,
/// parses the rest of the group exactly as [`brackets`] does.
///
/// Declines — `Ok(None)`, zero consumption — **iff the opener is definitely absent**: the
/// next valid token is not `[` (it stays unconsumed), or the input has genuinely ended. A
/// **terminal stop** at the would-be opener — a resource-limit trip, fresh or already
/// latched — is *neither*: it is not evidence the opener is absent, so the attempt does
/// not decline. It fails instead with the same end-of-input error the committed form
/// raises there (the trip's own diagnostic having already reached the emitter), so an
/// optional construct can never be silently skipped because the lexer was stopped rather
/// than satisfied. The moment the `[` opener is consumed the parse is **committed**:
/// `inner`'s errors and the missing/wrong `]` closer's diagnostics behave exactly as
/// [`brackets`]' do — an unterminated `[ …` reports the opener as [`Unclosed`] through the
/// emitter, never a silent decline (see [`try_delimited`] for why the attempt boundary is the
/// opener alone).
///
/// `inner` may be any [`ParseInput`](crate::ParseInput); the returned closure is itself
/// a `ParseInput` (yielding the `Option`) through the blanket impl.
///
/// # Examples
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug, PartialEq)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Digit(u32), LBracket, RBracket }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Digit, LBracket, RBracket }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> Kind { match self { Tok::Digit(_) => Kind::Digit, Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
/// #   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
/// # }
/// # impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
/// # impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
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
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c { '0'..='9' => Tok::Digit(c as u32 - '0' as u32), '[' => Tok::LBracket, ']' => Tok::RBracket, _ => Tok::Digit(0) }))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Digit(_)))? {
/// #     Some(sp) => match sp.data { Tok::Digit(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Parse, Parser, parser::try_brackets};
///
/// fn attempt<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Option<u32>, Error> {
///     try_brackets(digit)(inp).map(|d| d.map(|d| *d.data()))
/// }
///
/// assert_eq!(Parser::with_parser(attempt).parse_str("[1]").unwrap(), Some(1));
/// // No `[` opener: the attempt declines and the digit is left unconsumed.
/// assert_eq!(Parser::with_parser(attempt).parse_str("1").unwrap(), None);
/// // Opener consumed: committed — an unterminated group errors, it does not decline.
/// assert!(Parser::with_parser(attempt).parse_str("[1").is_err());
/// ```
#[inline]
pub fn try_brackets<'inp, L, Ctx, Lang, P, T, Cmpl>(
  mut inner: P,
) -> impl for<'c> FnMut(
  &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
) -> TryBracketsOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    let cursor = inp.cursor().clone();
    match OpenBracket::try_parse(inp)? {
      ParseAttempt::Accept(open) => finish_brackets(inp, &cursor, open, &mut inner).map(Some),
      ParseAttempt::Decline => Ok(None),
    }
  }
}

/// The result [`angles`] returns: `inner`'s output wrapped in an angle-delimited
/// [`Delimited`] spanning the whole `< … >`, or the propagated error.
pub type AnglesOf<'inp, L, Ctx, Lang, T> = Result<
  Delimited<
    OpenAngle<<L as Lexer<'inp>>::Span, (), Lang>,
    CloseAngle<<L as Lexer<'inp>>::Span, (), Lang>,
    T,
    <L as Lexer<'inp>>::Span,
  >,
  ErrorOf<'inp, L, Ctx, Lang>,
>;

/// Commits the `<` opener, runs `inner`, commits the `>` closer, and returns the
/// three as a [`Delimited`] whose span covers the whole `< … >`.
///
/// `inner` runs between the committed delimiters and its output becomes the
/// [`Delimited`] data. A missing closer reports the opener as [`Unclosed`] through the
/// emitter — a fail-fast emitter turns it into `Err`, a recovering emitter records it and
/// yields the `< …` group recovered with a synthesized `>`. A wrong token where `>` belongs
/// stays the unexpected-token (expected-close) diagnostic.
///
/// `inner` may be any [`ParseInput`](crate::ParseInput) — a closure or a named parser alike
/// — and the returned closure is itself a `ParseInput` through the blanket impl, so shapes
/// nest and compose without adapters.
///
/// # Examples
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug, PartialEq)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Digit(u32), LAngle, RAngle }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Digit, LAngle, RAngle }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> Kind { match self { Tok::Digit(_) => Kind::Digit, Tok::LAngle => Kind::LAngle, Tok::RAngle => Kind::RAngle } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_angle() -> Option<Kind> { Some(Kind::LAngle) }
/// #   fn close_angle() -> Option<Kind> { Some(Kind::RAngle) }
/// # }
/// # impl From<OpenAngle<(), (), ()>> for Kind { fn from(_: OpenAngle<(), (), ()>) -> Self { Kind::LAngle } }
/// # impl From<CloseAngle<(), (), ()>> for Kind { fn from(_: CloseAngle<(), (), ()>) -> Self { Kind::RAngle } }
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
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c { '0'..='9' => Tok::Digit(c as u32 - '0' as u32), '<' => Tok::LAngle, '>' => Tok::RAngle, _ => Tok::Digit(0) }))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Digit(_)))? {
/// #     Some(sp) => match sp.data { Tok::Digit(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Parse, Parser, parser::angles};
///
/// fn wrapped<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
///     angles(digit)(inp).map(|d| *d.data())
/// }
///
/// assert_eq!(Parser::with_parser(wrapped).parse_str("<1>").unwrap(), 1);
/// assert!(Parser::with_parser(wrapped).parse_str("<1").is_err());
/// ```
#[inline]
pub fn angles<'inp, L, Ctx, Lang, P, T, Cmpl>(
  mut inner: P,
) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>) -> AnglesOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    let cursor = inp.cursor().clone();
    let open = OpenAngle::parse(inp)?;
    finish_angles(inp, &cursor, open, &mut inner)
  }
}

/// The shared post-open body of [`angles`] and [`try_angles`]: from here the parse is
/// committed — runs `inner`, commits the `>` closer, and builds the [`Delimited`] whose
/// span runs from `cursor` (captured before the opener) to the closer.
#[inline]
fn finish_angles<'inp, 'c, L, Ctx, Lang, P, T, Cmpl>(
  inp: &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
  cursor: &Cursor<'inp, 'c, L>,
  open: OpenAngle<L::Span, (), Lang>,
  inner: &mut P,
) -> AnglesOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  let data = inner.parse_input(inp)?;
  let (close, span) = commit_delim_close::<Angle<(), (), Lang>, _, _, _, _, _>(
    inp,
    cursor,
    open.span(),
    PairIdentity::ANGLE,
    |t| t.is_close_angle(),
    |tok| SpannedPunctuatorToken::<'inp, L, Lang>::expect_close_angle(tok),
    |s| CloseAngle::new(s).change_language::<Lang>(),
  )?;
  Ok(Delimited::new(open, close, data, span))
}

/// The result the parser [`try_angles`] builds yields: `Some` of the angle-delimited
/// construct on accept, `None` on decline, or the propagated error.
pub type TryAnglesOf<'inp, L, Ctx, Lang, T> = Result<
  Option<
    Delimited<
      OpenAngle<<L as Lexer<'inp>>::Span, (), Lang>,
      CloseAngle<<L as Lexer<'inp>>::Span, (), Lang>,
      T,
      <L as Lexer<'inp>>::Span,
    >,
  >,
  ErrorOf<'inp, L, Ctx, Lang>,
>;

/// The attempt twin of [`angles`]: tries to commit the `<` opener and, if it is next,
/// parses the rest of the group exactly as [`angles`] does.
///
/// Declines — `Ok(None)`, zero consumption — **iff the opener is definitely absent**: the
/// next valid token is not `<` (it stays unconsumed), or the input has genuinely ended. A
/// **terminal stop** at the would-be opener — a resource-limit trip, fresh or already
/// latched — is *neither*: it is not evidence the opener is absent, so the attempt does
/// not decline. It fails instead with the same end-of-input error the committed form
/// raises there (the trip's own diagnostic having already reached the emitter), so an
/// optional construct can never be silently skipped because the lexer was stopped rather
/// than satisfied. The moment the `<` opener is consumed the parse is **committed**:
/// `inner`'s errors and the missing/wrong `>` closer's diagnostics behave exactly as
/// [`angles`]' do — an unterminated `< …` reports the opener as [`Unclosed`] through the
/// emitter, never a silent decline (see [`try_delimited`] for why the attempt boundary is the
/// opener alone).
///
/// `inner` may be any [`ParseInput`](crate::ParseInput); the returned closure is itself
/// a `ParseInput` (yielding the `Option`) through the blanket impl.
///
/// # Examples
///
/// ```rust
/// # use core::{convert::Infallible, fmt};
/// # use tokora::{
/// #   FatalContext, InputRef, Lexer, SimpleSpan, Token,
/// #   error::{Unclosed, UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
/// #   punct::{CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen},
/// #   span::Span as _,
/// #   token::PunctuatorToken,
/// # };
/// # #[derive(Debug, PartialEq)]
/// # struct Error;
/// # impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
/// # impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
/// # impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
/// # impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
/// # impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
/// # impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for Error { fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self { Error } }
/// # #[derive(Debug, Clone, PartialEq)]
/// # enum Tok { Digit(u32), LAngle, RAngle }
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// # enum Kind { Digit, LAngle, RAngle }
/// # impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
/// # impl Token<'_> for Tok {
/// #   type Kind = Kind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> Kind { match self { Tok::Digit(_) => Kind::Digit, Tok::LAngle => Kind::LAngle, Tok::RAngle => Kind::RAngle } }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # impl PunctuatorToken<'_> for Tok {
/// #   fn open_angle() -> Option<Kind> { Some(Kind::LAngle) }
/// #   fn close_angle() -> Option<Kind> { Some(Kind::RAngle) }
/// # }
/// # impl From<OpenAngle<(), (), ()>> for Kind { fn from(_: OpenAngle<(), (), ()>) -> Self { Kind::LAngle } }
/// # impl From<CloseAngle<(), (), ()>> for Kind { fn from(_: CloseAngle<(), (), ()>) -> Self { Kind::RAngle } }
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
/// #     let (start, c) = (self.pos, bytes[self.pos] as char);
/// #     self.pos += 1;
/// #     self.tok = SimpleSpan::new(start, self.pos);
/// #     Some(Ok(match c { '0'..='9' => Tok::Digit(c as u32 - '0' as u32), '<' => Tok::LAngle, '>' => Tok::RAngle, _ => Tok::Digit(0) }))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += n; }
/// # }
/// # type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
/// # fn digit<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<u32, Error> {
/// #   match inp.try_expect(|t| matches!(t.data, Tok::Digit(_)))? {
/// #     Some(sp) => match sp.data { Tok::Digit(n) => Ok(n), _ => unreachable!() },
/// #     None => Err(Error),
/// #   }
/// # }
/// use tokora::{Parse, Parser, parser::try_angles};
///
/// fn attempt<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Option<u32>, Error> {
///     try_angles(digit)(inp).map(|d| d.map(|d| *d.data()))
/// }
///
/// assert_eq!(Parser::with_parser(attempt).parse_str("<1>").unwrap(), Some(1));
/// // No `<` opener: the attempt declines and the digit is left unconsumed.
/// assert_eq!(Parser::with_parser(attempt).parse_str("1").unwrap(), None);
/// // Opener consumed: committed — an unterminated group errors, it does not decline.
/// assert!(Parser::with_parser(attempt).parse_str("<1").is_err());
/// ```
#[inline]
pub fn try_angles<'inp, L, Ctx, Lang, P, T, Cmpl>(
  mut inner: P,
) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>) -> TryAnglesOf<'inp, L, Ctx, Lang, T>
where
  L: Lexer<'inp>,
  L::Token: PunctuatorToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: ParseInput<'inp, L, T, Ctx, Lang, Cmpl>,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    let cursor = inp.cursor().clone();
    match OpenAngle::try_parse(inp)? {
      ParseAttempt::Accept(open) => finish_angles(inp, &cursor, open, &mut inner).map(Some),
      ParseAttempt::Decline => Ok(None),
    }
  }
}
