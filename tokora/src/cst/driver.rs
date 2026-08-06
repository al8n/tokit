//! The lossless parse entries: the **only** doors through which a [`Sink`] is created, driven,
//! and handed back.
//!
//! # Why a driver rather than a constructor
//!
//! A sink names the buffer its tree's text is sliced out of; an input names the buffer the parse
//! reads. While those were two independent names, one program could state them differently and
//! get a tree whose structure came from one source and whose text came from another — a pairing
//! nothing downstream can detect, because the event log it produces is byte-identical to a legal
//! one.
//!
//! These drivers take the source **once** and use it for both, so the two names are the same
//! argument of the same call. `Sink::new` is crate-private, so there is no other way to obtain
//! a sink; and the returned [`Cst`] implements no emitter trait, so the artefact cannot be
//! re-aimed at a second parse.
//!
//! # Why the emitter slot is pinned to `Sink` itself
//!
//! The `where` clause reads `P: ParseInput<'inp, L, O, (Sink<'inp, L, E>, C), Lang>` — the
//! context's emitter is `Sink<'inp, L, E>` *by name*, not "some emitter that forwards to a sink".
//! That is the whole mechanism. Every published bypass of the older finish-time wall had the same
//! shape — *put something of my own where the emitter goes*, and let it decide what to forward —
//! and every fix for one relocated the hole to the next, because the witness was state on the
//! sink and the sink cannot tell who set it. Pinning the slot removes the position all three
//! occupied, so they die together rather than one at a time: a wrapper `W(&mut Sink)` is not
//! `Sink`, and there is nowhere else for it to sit.

use crate::{
  Cache, Complete, Emitter, Lexer, ParseContext, ParseInput, Partial, SurfaceIncomplete,
  emitter::ValueKeyedEmitter, input::Input,
};

use super::{CstProfile, handle::Cst, sink::Sink};

/// Runs `f` over `src` with a **lossless recording sink** minted from that very source, and
/// returns the spent [`Cst`] alongside whatever `f` produced.
///
/// - `src` is the buffer both the parse and the tree's text come from — named once, so they
///   cannot disagree;
/// - `state` is the lexer's initial state;
/// - `inner` is the emitter every diagnostic forwards to (it must be
///   [`ValueKeyedEmitter`] — see the *Inner-emitter contract* on [`Sink`]);
/// - `profile` is the dialect's kind space ([`CstProfile`]): the token mapper into the unified
///   `u16` kind space, the [`KindValidator`](super::KindValidator) every recorded kind is checked
///   against, the `error_kind` wrapped around a recovery hole's skipped tokens, and the
///   `gap_kind` tiled over source bytes no committed token covers;
/// - `cache` is the lookahead cache, exactly as any other context supplies it;
/// - `f` is the parser — any [`ParseInput`] over the pinned context.
///
/// The result is returned **beside** the tree rather than through it: a parse that failed still
/// produced events worth materializing (that is what
/// [`finish_partial`](Cst::finish_partial) is for), so the handle is unconditional and the
/// `Result` is `f`'s own.
///
/// # The emitter slot is pinned
///
/// `Ctx::Emitter` is `Sink<'inp, L, E>` by name. A wrapper that forwards some emissions and
/// hides others cannot be installed here, because there is no slot for it to occupy — see the
/// module-level note for why that is what closes the class rather than another wall.
///
/// # Compile-time wall: trivia-surfacing lexers only
///
/// A lossless tree tiles every uncovered source byte, so it must be able to tell a byte the
/// lexer *refused* from a byte a committed token was *dropped* for. A trivia-skipping lexer
/// makes the two indistinguishable, so this door is restricted at **compile time** to lexers
/// declaring [`Lexer::SURFACES_TRIVIA`]. The wall is an inline-`const` assertion inside
/// `Sink::new`, and it fires through this call: a post-monomorphization `error[E0080]`
/// reported at the offending `parse_lossless` call site, with a note naming the instantiation.
/// It therefore fires at build/test/doc time — **not** under `cargo check`, which never
/// monomorphizes the call.
///
/// A lexer that surfaces trivia drives fine:
///
/// ```rust
/// use tokora::{
///   Emitter, InputRef, Lexer, ParseContext, SimpleSpan, Token,
///   cache::DefaultCache,
///   cst::{CstProfile, KindValidator, parse_lossless},
///   emitter::Verbose,
/// };
///
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # struct MiniErr;
/// # impl<'a, T, K: Clone, S, Lang: ?Sized>
/// #   From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>> for MiniErr
/// # {
/// #   fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>) -> Self { Self }
/// # }
/// #[derive(Debug, Clone, Copy)]
/// struct MiniTok(u8);
/// impl Token<'_> for MiniTok {
///   type Kind = u8;
///   type Error = MiniErr;
///   const SURFACES_TRIVIA: bool = true; // ← the declaration under test
///   fn kind(&self) -> u8 { self.0 }
///   fn is_trivia(&self) -> bool { self.0 == b' ' }
/// }
/// # struct Mini<'a> { src: &'a str, tok_start: usize, pos: usize, state: () }
/// # impl<'inp> Lexer<'inp> for Mini<'inp> {
/// #   type State = (); type Source = str; type Token = MiniTok;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'inp str) -> Self { Self { src, tok_start: 0, pos: 0, state: () } }
/// #   fn with_state(src: &'inp str, state: ()) -> Self {
/// #     Self { src, tok_start: 0, pos: 0, state }
/// #   }
/// #   fn check(&self) -> Result<(), MiniErr> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> () { self.state }
/// #   fn source(&self) -> &'inp str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.tok_start, self.pos) }
/// #   fn slice(&self) -> &'inp str { &self.src[self.tok_start..self.pos] }
/// #   fn lex(&mut self) -> Option<Result<MiniTok, MiniErr>> {
/// #     let byte = *self.src.as_bytes().get(self.pos)?;
/// #     self.tok_start = self.pos;
/// #     self.pos += 1;
/// #     Some(Ok(MiniTok(byte)))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += *n; self.tok_start = self.pos; }
/// # }
/// # const K_ROOT: u16 = 1;
/// # const K_TOK: u16 = 10;
/// # const K_ERR: u16 = 90;
/// # const K_GAP: u16 = 91;
/// /// Drain every token; the sink records each committed one.
/// fn drain<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Mini<'inp>, Ctx>) -> Result<usize, MiniErr>
/// where
///   Ctx: ParseContext<'inp, Mini<'inp>>,
///   Ctx::Emitter: Emitter<'inp, Mini<'inp>, (), Error = MiniErr>,
/// {
///   let mut n = 0;
///   while inp.next()?.is_some() {
///     n += 1;
///   }
///   Ok(n)
/// }
///
/// let src = "ab c";
/// let profile = CstProfile::new(
///   |_: &MiniTok| K_TOK,
///   KindValidator::new(|k| matches!(k, K_ROOT | K_TOK | K_ERR | K_GAP)),
///   K_ERR,
///   K_GAP,
/// );
///
/// let (cst, parsed) = parse_lossless(
///   src,
///   (),
///   Verbose::<MiniErr>::new(),
///   profile,
///   DefaultCache::<Mini<'_>>::new(),
///   drain,
/// );
/// assert_eq!(parsed, Ok(4));
///
/// let (green, _diagnostics) = cst.finish(K_ROOT);
/// // The round-trip law: the text came from the same buffer the parse read.
/// assert_eq!(green.unwrap().to_string(), src);
/// ```
///
/// The same driver with the `SURFACES_TRIVIA` declaration removed — the only difference — is
/// refused at monomorphization:
///
/// ```rust,compile_fail
/// # use tokora::{
/// #   Emitter, InputRef, Lexer, ParseContext, SimpleSpan, Token,
/// #   cache::DefaultCache,
/// #   cst::{CstProfile, KindValidator, parse_lossless},
/// #   emitter::Verbose,
/// # };
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # struct MiniErr;
/// # impl<'a, T, K: Clone, S, Lang: ?Sized>
/// #   From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>> for MiniErr
/// # {
/// #   fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>) -> Self { Self }
/// # }
/// #[derive(Debug, Clone, Copy)]
/// struct MiniTok(u8);
/// impl Token<'_> for MiniTok {
///   type Kind = u8;
///   type Error = MiniErr;
///   // no SURFACES_TRIVIA: defaults to false (a skipping, syntactic grammar)
///   fn kind(&self) -> u8 { self.0 }
///   fn is_trivia(&self) -> bool { self.0 == b' ' }
/// }
/// # struct Mini<'a> { src: &'a str, tok_start: usize, pos: usize, state: () }
/// # impl<'inp> Lexer<'inp> for Mini<'inp> {
/// #   type State = (); type Source = str; type Token = MiniTok;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'inp str) -> Self { Self { src, tok_start: 0, pos: 0, state: () } }
/// #   fn with_state(src: &'inp str, state: ()) -> Self {
/// #     Self { src, tok_start: 0, pos: 0, state }
/// #   }
/// #   fn check(&self) -> Result<(), MiniErr> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> () { self.state }
/// #   fn source(&self) -> &'inp str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.tok_start, self.pos) }
/// #   fn slice(&self) -> &'inp str { &self.src[self.tok_start..self.pos] }
/// #   fn lex(&mut self) -> Option<Result<MiniTok, MiniErr>> {
/// #     let byte = *self.src.as_bytes().get(self.pos)?;
/// #     self.tok_start = self.pos;
/// #     self.pos += 1;
/// #     Some(Ok(MiniTok(byte)))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += *n; self.tok_start = self.pos; }
/// # }
/// # const K_ROOT: u16 = 1;
/// # const K_TOK: u16 = 10;
/// # const K_ERR: u16 = 90;
/// # const K_GAP: u16 = 91;
/// # fn drain<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Mini<'inp>, Ctx>) -> Result<usize, MiniErr>
/// # where
/// #   Ctx: ParseContext<'inp, Mini<'inp>>,
/// #   Ctx::Emitter: Emitter<'inp, Mini<'inp>, (), Error = MiniErr>,
/// # {
/// #   let mut n = 0;
/// #   while inp.next()?.is_some() { n += 1; }
/// #   Ok(n)
/// # }
/// # let profile = CstProfile::new(
/// #   |_: &MiniTok| K_TOK,
/// #   KindValidator::new(|k| matches!(k, K_ROOT | K_TOK | K_ERR | K_GAP)),
/// #   K_ERR,
/// #   K_GAP,
/// # );
/// let (_cst, _parsed) = parse_lossless(
///   "ab c",
///   (),
///   Verbose::<MiniErr>::new(),
///   profile,
///   DefaultCache::<Mini<'_>>::new(),
///   drain,
/// );
/// ```
///
/// # Compile-time wall: value-keyed inner emitters only
///
/// The *Inner-emitter contract* on [`Sink`] is in the type system, and this is where it is
/// spent: `inner` must be a [`ValueKeyedEmitter`]. A **table-keyed** emitter — one that
/// allocates per-`checkpoint` bookkeeping behind interior mutability and expects to reclaim it
/// per-`release` — is refused here rather than accepted and then silently stranded (the sink's
/// `release` forwards nothing, so such an inner would leak one row per committed capture):
///
/// ```rust,compile_fail
/// use core::cell::RefCell;
/// use tokora::{
///   Emitter, InputRef, Lexer, ParseContext, SimpleSpan, Token,
///   cache::DefaultCache,
///   cst::{CstProfile, KindValidator, parse_lossless},
///   input::Cursor,
///   span::Spanned,
/// };
///
/// /// A table-keyed emitter: `checkpoint` allocates a row and hands back its index.
/// #[derive(Default)]
/// struct TableKeyed {
///   rows: RefCell<Vec<u64>>,
/// }
///
/// impl<'a, L, Lang: ?Sized> Emitter<'a, L, Lang> for TableKeyed {
///   type Error = MiniErr;
///   fn emit_lexer_error(
///     &mut self,
///     _e: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
///   ) -> Result<(), MiniErr> where L: Lexer<'a> { Ok(()) }
///   fn emit_unexpected_token(
///     &mut self,
///     _e: tokora::error::token::UnexpectedTokenOf<'a, L, Lang>,
///   ) -> Result<(), MiniErr> where L: Lexer<'a> { Ok(()) }
///   fn emit_error(&mut self, _e: Spanned<MiniErr, L::Span>) -> Result<(), MiniErr>
///   where L: Lexer<'a> { Ok(()) }
///   fn checkpoint(&self) -> u64 {
///     let mut rows = self.rows.borrow_mut();
///     rows.push(0);
///     rows.len() as u64 - 1 // a KEY, not a reading of the emission state
///   }
///   fn rewind(&mut self, _c: &Cursor<'a, '_, L>, _m: u64) where L: Lexer<'a> {}
/// }
///
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # struct MiniErr;
/// # #[derive(Debug, Clone, Copy)]
/// # struct MiniTok(u8);
/// # impl Token<'_> for MiniTok {
/// #   type Kind = u8;
/// #   type Error = MiniErr;
/// #   const SURFACES_TRIVIA: bool = true;
/// #   fn kind(&self) -> u8 { self.0 }
/// #   fn is_trivia(&self) -> bool { self.0 == b' ' }
/// # }
/// # struct Mini<'a> { src: &'a str, tok_start: usize, pos: usize, state: () }
/// # impl<'inp> Lexer<'inp> for Mini<'inp> {
/// #   type State = (); type Source = str; type Token = MiniTok;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'inp str) -> Self { Self { src, tok_start: 0, pos: 0, state: () } }
/// #   fn with_state(src: &'inp str, state: ()) -> Self {
/// #     Self { src, tok_start: 0, pos: 0, state }
/// #   }
/// #   fn check(&self) -> Result<(), MiniErr> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> () { self.state }
/// #   fn source(&self) -> &'inp str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.tok_start, self.pos) }
/// #   fn slice(&self) -> &'inp str { &self.src[self.tok_start..self.pos] }
/// #   fn lex(&mut self) -> Option<Result<MiniTok, MiniErr>> {
/// #     let byte = *self.src.as_bytes().get(self.pos)?;
/// #     self.tok_start = self.pos;
/// #     self.pos += 1;
/// #     Some(Ok(MiniTok(byte)))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += *n; self.tok_start = self.pos; }
/// # }
/// # const K_ROOT: u16 = 1;
/// # const K_TOK: u16 = 10;
/// # const K_ERR: u16 = 90;
/// # const K_GAP: u16 = 91;
/// # fn drain<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Mini<'inp>, Ctx>) -> Result<usize, MiniErr>
/// # where
/// #   Ctx: ParseContext<'inp, Mini<'inp>>,
/// #   Ctx::Emitter: Emitter<'inp, Mini<'inp>, (), Error = MiniErr>,
/// # {
/// #   let mut n = 0;
/// #   while inp.next()?.is_some() { n += 1; }
/// #   Ok(n)
/// # }
/// # let profile = CstProfile::new(
/// #   |_: &MiniTok| K_TOK,
/// #   KindValidator::new(|k| matches!(k, K_ROOT | K_TOK | K_ERR | K_GAP)),
/// #   K_ERR,
/// #   K_GAP,
/// # );
/// let (_cst, _parsed) = parse_lossless(
///   "ab c",
///   (),
///   TableKeyed::default(),
///   profile,
///   DefaultCache::<Mini<'_>>::new(),
///   drain,
/// );
/// ```
///
/// # The sink is not handed out by the handle
///
/// Pinning the slot closes the **entry**. It closes nothing on its own if the running parse then
/// hands the live sink back out, because `&mut Sink` *is* an emitter: a parser holding one can
/// wrap it and install the wrapper as the context of a second parse over a **different** buffer,
/// through any of the un-pinned entries ([`parse_with`](crate::parse_with),
/// [`parse_partial`](crate::parse_partial), the [`Parser`](crate::Parser) family). The sink then
/// records a foreign parse's structure and materializes it over its own bytes — and a wrapper
/// that declines to forward [`Emitter::bound_source`] gives up the runtime handshake that would
/// otherwise catch it.
///
/// So the **handle** exposes the emitter's operations and never the emitter:
/// [`InputRef::cst_mark`](crate::InputRef::cst_mark),
/// [`cst_start_at`](crate::InputRef::cst_start_at), [`cst_finish`](crate::InputRef::cst_finish),
/// [`emit_error`](crate::InputRef::emit_error) and the rest of the forwarded surface, plus
/// [`emitter_ref`](crate::InputRef::emitter_ref) to *read* the emitter. A method call is not a
/// value; there is nothing left at the end of one to put in an emitter slot.
///
/// ## And not by a callback parameter either
///
/// The handle was not the only door. Several public callback traits took the emitter as a
/// *parameter*, by `&mut`, and handed it to code the caller wrote:
/// `Decision::decide(peeked, emitter)` — the condition of every `*_while` combinator
/// ([`repeated_while`](crate::ParseInput::repeated_while),
/// [`separated_while`](crate::ParseInput::separated_while),
/// [`fold_while`](crate::ParseInput::fold_while)) — the `peek_then` family
/// ([`peek_then`](crate::ParseInput::peek_then),
/// [`peek_then_choice`](crate::ParseChoice::peek_then_choice)), and the token-level pratt folds
/// [`PrattFoldTokenPrefix`](crate::parser::PrattFoldTokenPrefix),
/// [`PrattFoldTokenInfix`](crate::parser::PrattFoldTokenInfix) and
/// [`PrattFoldTokenPostfix`](crate::parser::PrattFoldTokenPostfix). Under this driver
/// `Ctx::Emitter` *is* `Sink`, so an ordinary `*_while` condition used to be handed the live sink
/// by `&mut`, and the wrong-source class was reachable through it exactly as it had been through
/// the handle.
///
/// Every one of those now receives an [`EmitterView`](crate::EmitterView): the emitter's
/// operations, under the emitter's own names, in a value that implements no emitter trait. The
/// two `compile_fail` cells below are the old attack in both of its spellings — the parameter
/// typed as the sink, and the parameter typed as the view and then wrapped — and the runnable
/// cell after them is what a condition may still do, which is everything it could do before.
///
/// What follows is therefore a wall on the class, not only on the handle. It is **not** a claim
/// that the sink is unreachable by construction from outside the crate: see *What this does not
/// close* at the end of this section.
///
/// Wrapper emitters remain perfectly expressible, and installable — over an emitter the caller
/// owns:
///
/// ```rust
/// use tokora::{
///   Emitter, InputRef, Lexer, SimpleSpan, Token, cache::DefaultCache, input::Cursor,
///   parse_with, span::Spanned,
/// };
///
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # struct MiniErr;
/// # impl<'a, T, K: Clone, S, Lang: ?Sized>
/// #   From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>> for MiniErr
/// # {
/// #   fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>) -> Self { Self }
/// # }
/// # #[derive(Debug, Clone, Copy)]
/// # struct MiniTok(u8);
/// # impl Token<'_> for MiniTok {
/// #   type Kind = u8;
/// #   type Error = MiniErr;
/// #   const SURFACES_TRIVIA: bool = true;
/// #   fn kind(&self) -> u8 { self.0 }
/// #   fn is_trivia(&self) -> bool { self.0 == b' ' }
/// # }
/// # struct Mini<'a> { src: &'a str, tok_start: usize, pos: usize, state: () }
/// # impl<'inp> Lexer<'inp> for Mini<'inp> {
/// #   type State = (); type Source = str; type Token = MiniTok;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'inp str) -> Self { Self { src, tok_start: 0, pos: 0, state: () } }
/// #   fn with_state(src: &'inp str, state: ()) -> Self {
/// #     Self { src, tok_start: 0, pos: 0, state }
/// #   }
/// #   fn check(&self) -> Result<(), MiniErr> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> () { self.state }
/// #   fn source(&self) -> &'inp str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.tok_start, self.pos) }
/// #   fn slice(&self) -> &'inp str { &self.src[self.tok_start..self.pos] }
/// #   fn lex(&mut self) -> Option<Result<MiniTok, MiniErr>> {
/// #     let byte = *self.src.as_bytes().get(self.pos)?;
/// #     self.tok_start = self.pos;
/// #     self.pos += 1;
/// #     Some(Ok(MiniTok(byte)))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += *n; self.tok_start = self.pos; }
/// # }
/// /// A wrapper emitter: forwards every emission, and deliberately not `bound_source`.
/// struct Wrapper<E>(E);
///
/// type MSpan<'a> = <Mini<'a> as Lexer<'a>>::Span;
/// type MLexErr<'a> = <<Mini<'a> as Lexer<'a>>::Token as Token<'a>>::Error;
///
/// impl<'inp, E> Emitter<'inp, Mini<'inp>> for Wrapper<E>
/// where
///   E: Emitter<'inp, Mini<'inp>, (), Error = MiniErr>,
/// {
///   type Error = MiniErr;
///   fn emit_lexer_error(
///     &mut self,
///     e: Spanned<MLexErr<'inp>, MSpan<'inp>>,
///   ) -> Result<(), MiniErr> {
///     self.0.emit_lexer_error(e)
///   }
///   fn emit_unexpected_token(
///     &mut self,
///     e: tokora::error::token::UnexpectedTokenOf<'inp, Mini<'inp>, ()>,
///   ) -> Result<(), MiniErr> {
///     self.0.emit_unexpected_token(e)
///   }
///   fn emit_error(&mut self, e: Spanned<MiniErr, MSpan<'inp>>) -> Result<(), MiniErr> {
///     self.0.emit_error(e)
///   }
///   fn rewind(&mut self, cursor: &Cursor<'inp, '_, Mini<'inp>>, mark: u64) {
///     self.0.rewind(cursor, mark)
///   }
/// }
///
/// type WrapCtx<'inp> = (
///   Wrapper<tokora::emitter::Fatal<MiniErr>>,
///   DefaultCache<'inp, Mini<'inp>>,
/// );
///
/// fn drain<'inp>(inp: &mut InputRef<'inp, '_, Mini<'inp>, WrapCtx<'inp>>) -> Result<usize, MiniErr> {
///   let mut n = 0;
///   while inp.next()?.is_some() {
///     n += 1;
///   }
///   Ok(n)
/// }
///
/// let ctx: WrapCtx<'_> = (
///   Wrapper(tokora::emitter::Fatal::new()),
///   DefaultCache::<Mini<'_>>::new(),
/// );
/// assert_eq!(parse_with(drain, "ab", ctx), Ok(2));
/// ```
///
/// The parse's own sink is not reachable *through the handle*. The program below is the same
/// wrapper, installed into the same un-pinned entry, over a *foreign* buffer — and it is refused
/// at `inp.emitter()`, which is crate-private. (The callback route above is not refused; this
/// pair pins the handle, which is what it claims.)
///
/// ```rust,compile_fail
/// use tokora::{
///   Emitter, InputRef, Lexer, SimpleSpan, Token,
///   cache::DefaultCache,
///   cst::{CstProfile, KindValidator, Sink, parse_lossless},
///   emitter::Verbose,
///   input::Cursor,
///   parse_with,
///   span::Spanned,
/// };
///
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # struct MiniErr;
/// # impl<'a, T, K: Clone, S, Lang: ?Sized>
/// #   From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>> for MiniErr
/// # {
/// #   fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>) -> Self { Self }
/// # }
/// # #[derive(Debug, Clone, Copy)]
/// # struct MiniTok(u8);
/// # impl Token<'_> for MiniTok {
/// #   type Kind = u8;
/// #   type Error = MiniErr;
/// #   const SURFACES_TRIVIA: bool = true;
/// #   fn kind(&self) -> u8 { self.0 }
/// #   fn is_trivia(&self) -> bool { self.0 == b' ' }
/// # }
/// # struct Mini<'a> { src: &'a str, tok_start: usize, pos: usize, state: () }
/// # impl<'inp> Lexer<'inp> for Mini<'inp> {
/// #   type State = (); type Source = str; type Token = MiniTok;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'inp str) -> Self { Self { src, tok_start: 0, pos: 0, state: () } }
/// #   fn with_state(src: &'inp str, state: ()) -> Self {
/// #     Self { src, tok_start: 0, pos: 0, state }
/// #   }
/// #   fn check(&self) -> Result<(), MiniErr> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> () { self.state }
/// #   fn source(&self) -> &'inp str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.tok_start, self.pos) }
/// #   fn slice(&self) -> &'inp str { &self.src[self.tok_start..self.pos] }
/// #   fn lex(&mut self) -> Option<Result<MiniTok, MiniErr>> {
/// #     let byte = *self.src.as_bytes().get(self.pos)?;
/// #     self.tok_start = self.pos;
/// #     self.pos += 1;
/// #     Some(Ok(MiniTok(byte)))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += *n; self.tok_start = self.pos; }
/// # }
/// # const K_ROOT: u16 = 1;
/// # const K_TOK: u16 = 10;
/// # const K_ERR: u16 = 90;
/// # const K_GAP: u16 = 91;
/// struct Wrapper<E>(E);
///
/// type MSpan<'a> = <Mini<'a> as Lexer<'a>>::Span;
/// type MLexErr<'a> = <<Mini<'a> as Lexer<'a>>::Token as Token<'a>>::Error;
///
/// impl<'inp, E> Emitter<'inp, Mini<'inp>> for Wrapper<E>
/// where
///   E: Emitter<'inp, Mini<'inp>, (), Error = MiniErr>,
/// {
///   type Error = MiniErr;
///   fn emit_lexer_error(
///     &mut self,
///     e: Spanned<MLexErr<'inp>, MSpan<'inp>>,
///   ) -> Result<(), MiniErr> {
///     self.0.emit_lexer_error(e)
///   }
///   fn emit_unexpected_token(
///     &mut self,
///     e: tokora::error::token::UnexpectedTokenOf<'inp, Mini<'inp>, ()>,
///   ) -> Result<(), MiniErr> {
///     self.0.emit_unexpected_token(e)
///   }
///   fn emit_error(&mut self, e: Spanned<MiniErr, MSpan<'inp>>) -> Result<(), MiniErr> {
///     self.0.emit_error(e)
///   }
///   fn rewind(&mut self, cursor: &Cursor<'inp, '_, Mini<'inp>>, mark: u64) {
///     self.0.rewind(cursor, mark)
///   }
///   // `bound_source` deliberately NOT forwarded: the runtime handshake never fires.
/// }
///
/// type LosslessCtx<'inp> = (
///   Sink<'inp, Mini<'inp>, Verbose<MiniErr>>,
///   DefaultCache<'inp, Mini<'inp>>,
/// );
/// type ForeignCtx<'inp, 'e> = (
///   Wrapper<&'e mut Sink<'inp, Mini<'inp>, Verbose<MiniErr>>>,
///   DefaultCache<'inp, Mini<'inp>>,
/// );
///
/// fn drain<'inp>(
///   inp: &mut InputRef<'inp, '_, Mini<'inp>, ForeignCtx<'inp, '_>>,
/// ) -> Result<usize, MiniErr> {
///   let mut n = 0;
///   while inp.next()?.is_some() {
///     n += 1;
///   }
///   Ok(n)
/// }
///
/// fn attack<'inp>(
///   inp: &mut InputRef<'inp, '_, Mini<'inp>, LosslessCtx<'inp>>,
/// ) -> Result<usize, MiniErr> {
///   // THE LINE. `InputRef::emitter` is crate-private, so the live sink never becomes a value.
///   let sink: &mut Sink<'inp, Mini<'inp>, Verbose<MiniErr>> = inp.emitter();
///   let ctx: ForeignCtx<'inp, '_> = (Wrapper(sink), DefaultCache::<Mini<'_>>::new());
///   // "ZZ" is a foreign buffer of the same length as the sink's own.
///   parse_with(drain, "ZZ", ctx)
/// }
///
/// # let profile = CstProfile::new(
/// #   |_: &MiniTok| K_TOK,
/// #   KindValidator::new(|k| matches!(k, K_ROOT | K_TOK | K_ERR | K_GAP)),
/// #   K_ERR,
/// #   K_GAP,
/// # );
/// let (cst, parsed) = parse_lossless(
///   "ab",
///   (),
///   Verbose::<MiniErr>::new(),
///   profile,
///   DefaultCache::<Mini<'_>>::new(),
///   attack,
/// );
/// assert_eq!(parsed, Ok(2));
/// // Would have been: structure from "ZZ", text from "ab".
/// let (green, _diagnostics) = cst.finish(K_ROOT);
/// assert_eq!(green.unwrap().to_string(), "ab");
/// ```
///
/// ### The callback route, refused at the signature
///
/// This is `a_wrapper_around_the_live_sink_still_reaches_a_callback_parameter` — the in-crate cell
/// that pinned this hole while it was open — transcribed onto the public surface. An ordinary
/// `repeated_while` condition asks for the live sink by `&mut`, wraps it in a
/// `bound_source`-hiding emitter, and drives the foreign buffer `"ZZ"` through the un-pinned
/// [`parse_with`](crate::parse_with). It used to compile, run, and materialize `"ZZ"`'s structure
/// over `"ab"`'s bytes. The condition's second parameter is no longer `&mut Sink`, so it is
/// refused before any of that:
///
/// ```rust,compile_fail
/// use tokora::{
///   Accumulator, Emitter, InputRef, Lexer, ParseInput, SimpleSpan, Token,
///   cache::{DefaultCache, Peeked},
///   cst::{CstProfile, KindValidator, Sink, parse_lossless},
///   emitter::Verbose,
///   input::Cursor,
///   parse_with,
///   parser::Action,
///   span::Spanned,
///   utils::typenum::U1,
/// };
///
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # struct MiniErr;
/// # impl<'a, T, K: Clone, S, Lang: ?Sized>
/// #   From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>> for MiniErr
/// # {
/// #   fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>) -> Self { Self }
/// # }
/// #[derive(Debug, Clone, Copy)]
/// struct MiniTok(u8);
/// impl Token<'_> for MiniTok {
///   type Kind = u8;
///   type Error = MiniErr;
///   const SURFACES_TRIVIA: bool = true; // ← the declaration under test
///   fn kind(&self) -> u8 { self.0 }
///   fn is_trivia(&self) -> bool { self.0 == b' ' }
/// }
/// # struct Mini<'a> { src: &'a str, tok_start: usize, pos: usize, state: () }
/// # impl<'inp> Lexer<'inp> for Mini<'inp> {
/// #   type State = (); type Source = str; type Token = MiniTok;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'inp str) -> Self { Self { src, tok_start: 0, pos: 0, state: () } }
/// #   fn with_state(src: &'inp str, state: ()) -> Self {
/// #     Self { src, tok_start: 0, pos: 0, state }
/// #   }
/// #   fn check(&self) -> Result<(), MiniErr> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> () { self.state }
/// #   fn source(&self) -> &'inp str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.tok_start, self.pos) }
/// #   fn slice(&self) -> &'inp str { &self.src[self.tok_start..self.pos] }
/// #   fn lex(&mut self) -> Option<Result<MiniTok, MiniErr>> {
/// #     let byte = *self.src.as_bytes().get(self.pos)?;
/// #     self.tok_start = self.pos;
/// #     self.pos += 1;
/// #     Some(Ok(MiniTok(byte)))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += *n; self.tok_start = self.pos; }
/// # }
/// # const K_ROOT: u16 = 1;
/// # const K_TOK: u16 = 10;
/// # const K_ERR: u16 = 90;
/// # const K_GAP: u16 = 91;
/// struct Wrapper<E>(E);
///
/// type MSpan<'a> = <Mini<'a> as Lexer<'a>>::Span;
/// type MLexErr<'a> = <<Mini<'a> as Lexer<'a>>::Token as Token<'a>>::Error;
///
/// impl<'inp, E> Emitter<'inp, Mini<'inp>> for Wrapper<E>
/// where
///   E: Emitter<'inp, Mini<'inp>, (), Error = MiniErr>,
/// {
///   type Error = MiniErr;
///   fn emit_lexer_error(
///     &mut self,
///     e: Spanned<MLexErr<'inp>, MSpan<'inp>>,
///   ) -> Result<(), MiniErr> {
///     self.0.emit_lexer_error(e)
///   }
///   fn emit_unexpected_token(
///     &mut self,
///     e: tokora::error::token::UnexpectedTokenOf<'inp, Mini<'inp>, ()>,
///   ) -> Result<(), MiniErr> {
///     self.0.emit_unexpected_token(e)
///   }
///   fn emit_error(&mut self, e: Spanned<MiniErr, MSpan<'inp>>) -> Result<(), MiniErr> {
///     self.0.emit_error(e)
///   }
///   fn rewind(&mut self, cursor: &Cursor<'inp, '_, Mini<'inp>>, mark: u64) {
///     self.0.rewind(cursor, mark)
///   }
///   // `bound_source` deliberately NOT forwarded: the runtime handshake never fires.
/// }
///
/// type MiniSink<'inp> = Sink<'inp, Mini<'inp>, Verbose<MiniErr>>;
/// type LosslessCtx<'inp> = (MiniSink<'inp>, DefaultCache<'inp, Mini<'inp>>);
/// type ForeignCtx<'inp, 'e> = (
///   Wrapper<&'e mut MiniSink<'inp>>,
///   DefaultCache<'inp, Mini<'inp>>,
/// );
///
/// fn foreign<'inp>(
///   inp: &mut InputRef<'inp, '_, Mini<'inp>, ForeignCtx<'inp, '_>>,
/// ) -> Result<usize, MiniErr> {
///   let mut n = 0;
///   while inp.next()?.is_some() {
///     n += 1;
///   }
///   Ok(n)
/// }
///
/// fn step<'inp>(
///   inp: &mut InputRef<'inp, '_, Mini<'inp>, LosslessCtx<'inp>>,
/// ) -> Result<(), MiniErr> {
///   let _ = inp.next()?;
///   Ok(())
/// }
///
/// // THE SIGNATURE. A `*_while` condition asking for the live sink by `&mut` is not a
/// // `Decision` any more: the second parameter is an `EmitterView`, which is not an emitter.
/// fn decide<'inp>(
///   _peeked: Peeked<'_, 'inp, Mini<'inp>, U1>,
///   emitter: &mut MiniSink<'inp>,
/// ) -> Result<Action, MiniErr> {
///   let ctx: ForeignCtx<'inp, '_> = (Wrapper(emitter), DefaultCache::<Mini<'_>>::new());
///   // "ZZ" is a foreign buffer of the same length as the sink's own "ab".
///   let _ = parse_with(foreign, "ZZ", ctx);
///   Ok(Action::Stop)
/// }
///
/// fn attack<'inp>(
///   inp: &mut InputRef<'inp, '_, Mini<'inp>, LosslessCtx<'inp>>,
/// ) -> Result<Vec<()>, MiniErr> {
///   (step as fn(&mut InputRef<'inp, '_, Mini<'inp>, LosslessCtx<'inp>>) -> _)
///     .repeated_while::<_, U1>(
///       decide as fn(Peeked<'_, 'inp, Mini<'inp>, U1>, &mut MiniSink<'inp>) -> _,
///     )
///     .collect()
///     .parse_input(inp)
/// }
///
/// # let profile = CstProfile::new(
/// #   |_: &MiniTok| K_TOK,
/// #   KindValidator::new(|k| matches!(k, K_ROOT | K_TOK | K_ERR | K_GAP)),
/// #   K_ERR,
/// #   K_GAP,
/// # );
/// let (_cst, _parsed) = parse_lossless(
///   "ab",
///   (),
///   Verbose::<MiniErr>::new(),
///   profile,
///   DefaultCache::<Mini<'_>>::new(),
///   attack,
/// );
/// ```
///
/// ### The same attack, with the parameter spelled correctly
///
/// Renaming a parameter type closes nothing on its own; the wall is that the value handed over
/// **is not an emitter**. So here is the identical attack with the condition's signature written
/// the way the trait now demands — an [`EmitterView`](crate::EmitterView) — trying to install
/// that view instead. `Wrapper<E>` is an emitter only when `E` is one, and a view never is, so
/// the context cannot be built:
///
/// ```rust,compile_fail
/// use tokora::{
///   Accumulator, Emitter, EmitterView, InputRef, Lexer, ParseInput, SimpleSpan, Token,
///   cache::{DefaultCache, Peeked},
///   cst::{CstProfile, KindValidator, Sink, parse_lossless},
///   emitter::Verbose,
///   input::Cursor,
///   parse_with,
///   parser::Action,
///   span::Spanned,
///   utils::typenum::U1,
/// };
///
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # struct MiniErr;
/// # impl<'a, T, K: Clone, S, Lang: ?Sized>
/// #   From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>> for MiniErr
/// # {
/// #   fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>) -> Self { Self }
/// # }
/// #[derive(Debug, Clone, Copy)]
/// struct MiniTok(u8);
/// impl Token<'_> for MiniTok {
///   type Kind = u8;
///   type Error = MiniErr;
///   const SURFACES_TRIVIA: bool = true; // ← the declaration under test
///   fn kind(&self) -> u8 { self.0 }
///   fn is_trivia(&self) -> bool { self.0 == b' ' }
/// }
/// # struct Mini<'a> { src: &'a str, tok_start: usize, pos: usize, state: () }
/// # impl<'inp> Lexer<'inp> for Mini<'inp> {
/// #   type State = (); type Source = str; type Token = MiniTok;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'inp str) -> Self { Self { src, tok_start: 0, pos: 0, state: () } }
/// #   fn with_state(src: &'inp str, state: ()) -> Self {
/// #     Self { src, tok_start: 0, pos: 0, state }
/// #   }
/// #   fn check(&self) -> Result<(), MiniErr> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> () { self.state }
/// #   fn source(&self) -> &'inp str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.tok_start, self.pos) }
/// #   fn slice(&self) -> &'inp str { &self.src[self.tok_start..self.pos] }
/// #   fn lex(&mut self) -> Option<Result<MiniTok, MiniErr>> {
/// #     let byte = *self.src.as_bytes().get(self.pos)?;
/// #     self.tok_start = self.pos;
/// #     self.pos += 1;
/// #     Some(Ok(MiniTok(byte)))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += *n; self.tok_start = self.pos; }
/// # }
/// # const K_ROOT: u16 = 1;
/// # const K_TOK: u16 = 10;
/// # const K_ERR: u16 = 90;
/// # const K_GAP: u16 = 91;
/// struct Wrapper<E>(E);
///
/// type MSpan<'a> = <Mini<'a> as Lexer<'a>>::Span;
/// type MLexErr<'a> = <<Mini<'a> as Lexer<'a>>::Token as Token<'a>>::Error;
///
/// impl<'inp, E> Emitter<'inp, Mini<'inp>> for Wrapper<E>
/// where
///   E: Emitter<'inp, Mini<'inp>, (), Error = MiniErr>,
/// {
///   type Error = MiniErr;
///   fn emit_lexer_error(
///     &mut self,
///     e: Spanned<MLexErr<'inp>, MSpan<'inp>>,
///   ) -> Result<(), MiniErr> {
///     self.0.emit_lexer_error(e)
///   }
///   fn emit_unexpected_token(
///     &mut self,
///     e: tokora::error::token::UnexpectedTokenOf<'inp, Mini<'inp>, ()>,
///   ) -> Result<(), MiniErr> {
///     self.0.emit_unexpected_token(e)
///   }
///   fn emit_error(&mut self, e: Spanned<MiniErr, MSpan<'inp>>) -> Result<(), MiniErr> {
///     self.0.emit_error(e)
///   }
///   fn rewind(&mut self, cursor: &Cursor<'inp, '_, Mini<'inp>>, mark: u64) {
///     self.0.rewind(cursor, mark)
///   }
///   // `bound_source` deliberately NOT forwarded: the runtime handshake never fires.
/// }
///
/// type MiniSink<'inp> = Sink<'inp, Mini<'inp>, Verbose<MiniErr>>;
/// type LosslessCtx<'inp> = (MiniSink<'inp>, DefaultCache<'inp, Mini<'inp>>);
/// // The context the attack wants to build: a wrapper around the VIEW rather than the sink.
/// type ForeignCtx<'a, 'inp> = (
///   Wrapper<EmitterView<'a, 'inp, Mini<'inp>, MiniSink<'inp>>>,
///   DefaultCache<'inp, Mini<'inp>>,
/// );
///
/// fn foreign<'inp>(
///   inp: &mut InputRef<'inp, '_, Mini<'inp>, ForeignCtx<'_, 'inp>>,
/// ) -> Result<usize, MiniErr> {
///   let mut n = 0;
///   while inp.next()?.is_some() {
///     n += 1;
///   }
///   Ok(n)
/// }
///
/// fn step<'inp>(
///   inp: &mut InputRef<'inp, '_, Mini<'inp>, LosslessCtx<'inp>>,
/// ) -> Result<(), MiniErr> {
///   let _ = inp.next()?;
///   Ok(())
/// }
///
/// fn decide<'inp>(
///   _peeked: Peeked<'_, 'inp, Mini<'inp>, U1>,
///   emitter: EmitterView<'_, 'inp, Mini<'inp>, MiniSink<'inp>>,
/// ) -> Result<Action, MiniErr> {
///   // THE LINE. `Wrapper<E>` needs `E: Emitter`, and an `EmitterView` implements no emitter
///   // trait — so this tuple is not a `ParseContext` and there is nothing to install.
///   let ctx: ForeignCtx<'_, 'inp> = (Wrapper(emitter), DefaultCache::<Mini<'_>>::new());
///   let _ = parse_with(foreign, "ZZ", ctx);
///   Ok(Action::Stop)
/// }
///
/// fn attack<'inp>(
///   inp: &mut InputRef<'inp, '_, Mini<'inp>, LosslessCtx<'inp>>,
/// ) -> Result<Vec<()>, MiniErr> {
///   (step as fn(&mut InputRef<'inp, '_, Mini<'inp>, LosslessCtx<'inp>>) -> _)
///     .repeated_while::<_, U1>(
///       decide
///         as fn(
///           Peeked<'_, 'inp, Mini<'inp>, U1>,
///           EmitterView<'_, 'inp, Mini<'inp>, MiniSink<'inp>>,
///         ) -> _,
///     )
///     .collect()
///     .parse_input(inp)
/// }
///
/// # let profile = CstProfile::new(
/// #   |_: &MiniTok| K_TOK,
/// #   KindValidator::new(|k| matches!(k, K_ROOT | K_TOK | K_ERR | K_GAP)),
/// #   K_ERR,
/// #   K_GAP,
/// # );
/// let (_cst, _parsed) = parse_lossless(
///   "ab",
///   (),
///   Verbose::<MiniErr>::new(),
///   profile,
///   DefaultCache::<Mini<'_>>::new(),
///   attack,
/// );
/// ```
///
/// ### What a condition may still do: decide *and* report, without a rewind
///
/// The point of [`Decision`](crate::Decision) is peek → decide → act with no restore, and
/// reporting inline is half of that. The view carries every emitting operation the emitter had,
/// under the emitter's own names, so a condition that used to write `emitter.emit_warning(..)`
/// writes exactly that still — only the parameter's type moved. The condition below reports
/// through the view, opens and closes a CST node through it, and returns an
/// [`Action`](crate::parser::Action), in one pass over the peeked window; the drive then carries
/// on from where the peek left the input, with nothing rolled back:
///
/// ```rust
/// use tokora::{
///   CstEmitter, Decision, Emitter, EmitterView, InputRef, Lexer, ParseContext, SimpleSpan, Token,
///   cache::{DefaultCache, Peeked},
///   cst::{CstProfile, KindValidator, Sink, parse_lossless},
///   emitter::Verbose,
///   parser::Action,
///   span::Spanned,
///   utils::typenum::U1,
/// };
///
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # struct MiniErr;
/// # impl<'a, T, K: Clone, S, Lang: ?Sized>
/// #   From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>> for MiniErr
/// # {
/// #   fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>) -> Self { Self }
/// # }
/// #[derive(Debug, Clone, Copy)]
/// struct MiniTok(u8);
/// impl Token<'_> for MiniTok {
///   type Kind = u8;
///   type Error = MiniErr;
///   const SURFACES_TRIVIA: bool = true; // ← the declaration under test
///   fn kind(&self) -> u8 { self.0 }
///   fn is_trivia(&self) -> bool { self.0 == b' ' }
/// }
/// # struct Mini<'a> { src: &'a str, tok_start: usize, pos: usize, state: () }
/// # impl<'inp> Lexer<'inp> for Mini<'inp> {
/// #   type State = (); type Source = str; type Token = MiniTok;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'inp str) -> Self { Self { src, tok_start: 0, pos: 0, state: () } }
/// #   fn with_state(src: &'inp str, state: ()) -> Self {
/// #     Self { src, tok_start: 0, pos: 0, state }
/// #   }
/// #   fn check(&self) -> Result<(), MiniErr> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> () { self.state }
/// #   fn source(&self) -> &'inp str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.tok_start, self.pos) }
/// #   fn slice(&self) -> &'inp str { &self.src[self.tok_start..self.pos] }
/// #   fn lex(&mut self) -> Option<Result<MiniTok, MiniErr>> {
/// #     let byte = *self.src.as_bytes().get(self.pos)?;
/// #     self.tok_start = self.pos;
/// #     self.pos += 1;
/// #     Some(Ok(MiniTok(byte)))
/// #   }
/// #   fn bump(&mut self, n: &usize) { self.pos += *n; self.tok_start = self.pos; }
/// # }
/// # const K_ROOT: u16 = 1;
/// # const K_TOK: u16 = 10;
/// # const K_ERR: u16 = 90;
/// # const K_GAP: u16 = 91;
/// const K_NOTE: u16 = 11;
///
/// type MiniSink<'inp> = Sink<'inp, Mini<'inp>, Verbose<MiniErr>>;
/// type LosslessCtx<'inp> = (MiniSink<'inp>, DefaultCache<'inp, Mini<'inp>>);
///
/// fn step<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Mini<'inp>, Ctx>) -> Result<(), MiniErr>
/// where
///   Ctx: ParseContext<'inp, Mini<'inp>>,
///   Ctx::Emitter: Emitter<'inp, Mini<'inp>, Error = MiniErr>,
/// {
///   let _ = inp.next()?;
///   Ok(())
/// }
///
/// // An ordinary `*_while` condition: continue while the head is `a`; on anything else report
/// // and stop — inline, no rewind.
/// fn decide<'inp, Ctx>(
///   mut peeked: Peeked<'_, 'inp, Mini<'inp>, U1>,
///   mut emitter: EmitterView<'_, 'inp, Mini<'inp>, Ctx::Emitter>,
/// ) -> Result<Action, MiniErr>
/// where
///   Ctx: ParseContext<'inp, Mini<'inp>>,
///   Ctx::Emitter: Emitter<'inp, Mini<'inp>, Error = MiniErr> + CstEmitter<'inp, Mini<'inp>>,
/// {
///   use tokora::cache::PeekedTokenExt as _;
///   match peeked.pop_front() {
///     Some(t) if t.token().kind() == b'a' => Ok(Action::Continue),
///     Some(t) => {
///       let span = *t.span();
///       // Every emitting channel the emitter had, under the same name.
///       emitter.emit_warning(Spanned::new(span, MiniErr))?;
///       // The CST structuring surface too — the same one the handle forwards.
///       emitter.cst_start(K_NOTE);
///       emitter.cst_finish(K_NOTE);
///       Ok(Action::Stop)
///     }
///     None => Ok(Action::Stop),
///   }
/// }
///
/// // Driven as a `Decision` — the trait every `*_while` combinator bounds its condition by.
/// fn run<'inp>(
///   inp: &mut InputRef<'inp, '_, Mini<'inp>, LosslessCtx<'inp>>,
/// ) -> Result<usize, MiniErr> {
///   let mut taken = 0;
///   loop {
///     let action = {
///       let (peeked, view) = inp.peek_with_emitter::<U1>()?;
///       let mut cond = decide::<LosslessCtx<'inp>>;
///       Decision::<Mini<'inp>, MiniSink<'inp>, U1>::decide(&mut cond, peeked, view)?
///     };
///     match action {
///       Action::Continue => {
///         step(inp)?;
///         taken += 1;
///       }
///       Action::Stop => break,
///     }
///   }
///   // Nothing was rewound: the parse carries on over the rest of its own buffer.
///   while inp.next()?.is_some() {}
///   Ok(taken)
/// }
///
/// let profile = CstProfile::new(
///   |_: &MiniTok| K_TOK,
///   KindValidator::new(|k| matches!(k, K_ROOT | K_TOK | K_NOTE | K_ERR | K_GAP)),
///   K_ERR,
///   K_GAP,
/// );
/// let (cst, parsed) = parse_lossless(
///   "aab",
///   (),
///   Verbose::<MiniErr>::new(),
///   profile,
///   DefaultCache::<Mini<'_>>::new(),
///   run,
/// );
/// assert_eq!(parsed, Ok(2));
///
/// let (green, diagnostics) = cst.finish(K_ROOT);
/// // The report the condition raised inline is in the log ...
/// assert_eq!(diagnostics.warnings().len(), 1);
/// // ... and the round-trip law still holds: the text is the buffer the parse read.
/// assert_eq!(green.unwrap().to_string(), "aab");
/// ```
///
/// ### What this does not close
///
/// [`EmitterView`](crate::EmitterView) implements no emitter trait *in this crate*, and that is
/// what the two cells above pin. It is not a proof that no such implementation can exist: a
/// downstream crate whose own lexer type appears in the parameter list may write
/// `impl Emitter<'_, MyLexer<'_>> for EmitterView<'_, '_, MyLexer<'_>, Sink<..>>` within the
/// orphan rules, and install *that* over a foreign buffer.
///
/// What such an implementation can forward is bounded by the view's surface, and **both** members
/// that decide what a source byte is are off it:
///
/// - [`Emitter::commit_token`] — the auto-emission chokepoint, and since `CstEmitter::cst_token`
///   was deleted the only producer of token events. A foreign parse driven this way contributes
///   no token, no span and no byte of its own buffer.
/// - [`Emitter::commit_lexer_error`] — the only producer of **gap-coverage evidence**. Stating
///   the guarantee in terms of `commit_token` alone was incomplete for one round: a recorded
///   lexer-error span *licenses* an untokenized byte, and a foreign parse's input layer reports
///   its own refusals, in its own buffer's coordinates. Through a wrapper that forwarded them,
///   those spans excused bytes of *this* sink's source that this parse never covered.
///   The view's [`emit_lexer_error`](crate::EmitterView::emit_lexer_error) records the diagnostic
///   and no span, so the route now carries a report and no licence. Pinned end to end, from
///   outside the crate, by `an_orphan_view_wrapper_carries_a_foreign_lexer_error_but_licenses_no_gap`
///   in `tests/parser_node.rs`.
///
/// What is left reachable is the node and diagnostic channels, which the condition could already
/// reach directly and by design. The wrong-*text* class the drivers exist to close has no route
/// through it: a materialized tree's text is this buffer, covered by this parse's own tokens and
/// explained by this parse's own lexer's refusals, or `finish` refuses it.
#[inline]
pub fn parse_lossless<'inp, L, Lang, E, C, O, P>(
  src: &'inp L::Source,
  state: L::State,
  inner: E,
  profile: CstProfile<L::Token>,
  cache: C,
  mut f: P,
) -> (
  Cst<'inp, L, E>,
  Result<O, <E as Emitter<'inp, L, Lang>>::Error>,
)
where
  L: Lexer<'inp>,
  Lang: ?Sized,
  E: Emitter<'inp, L, Lang> + ValueKeyedEmitter,
  C: Cache<'inp, L, Lang>,
  P: ParseInput<'inp, L, O, (Sink<'inp, L, E>, C), Lang>,
{
  let ctx = (Sink::new(src, inner, profile), cache);
  let mut input = Input::<L, (Sink<'inp, L, E>, C), Lang, Complete>::with_state_and_context(
    src,
    state,
    ParseContext::provide(ctx),
  );
  let parsed = f.parse_input(&mut input.as_ref());
  // Read the descent-trip count BEFORE `into_emitter`, which drops the input and every cell that
  // is not the emitter. That line is where this fact used to die: the trip is deliberately not an
  // emitter event, so a consumer that keeps the tree and the diagnostics and discards the `Result`
  // — the ordinary lossless shape — had nothing left to ask.
  let resource_trips = input.resource_trips();
  (Cst::from_sink(input.into_emitter(), resource_trips), parsed)
}

/// The [`Partial`] sibling of [`parse_lossless`]: the same pinned sink, driven in Sans-I/O
/// partial mode.
///
/// `is_final` states whether `src` is the last chunk, exactly as
/// [`parse_partial`](crate::parse_partial)'s does; while it is `false` the
/// [frontier rules](crate::input) are active, so consuming across the end of the buffer
/// surfaces an [`Incomplete`](crate::error::Incomplete) on the `Err` channel instead of an
/// end-of-input. Everything else — the pinned emitter slot, the source named once, the returned
/// [`Cst`] — is identical to the complete driver.
///
/// # Materializing an incomplete parse
///
/// Use [`finish_partial`](Cst::finish_partial), not [`finish`](Cst::finish): a truncated parse
/// leaves open nodes and an un-diagnosed tail, and those are the two signals the partial door
/// tolerates. A parse that ended `Incomplete` and will be *retried* over a longer buffer should
/// materialize neither — drop the handle and drive again over the larger slice, exactly as
/// [`parse_partial`](crate::parse_partial)'s refill loop does.
///
/// # Cost
///
/// Each call builds a fresh input **and a fresh sink** over the current buffer, so a refill loop
/// re-lexes and re-records the whole prefix every attempt: cumulative work is
/// **Θ(Σ attempt lengths)**, the same triangular figure
/// [`parse_partial`](crate::parse_partial) documents, with the event log on top of the lexing.
#[inline]
pub fn parse_lossless_partial<'inp, L, Lang, E, C, O, P>(
  src: &'inp L::Source,
  state: L::State,
  inner: E,
  profile: CstProfile<L::Token>,
  cache: C,
  is_final: bool,
  mut f: P,
) -> (
  Cst<'inp, L, E>,
  Result<O, <E as Emitter<'inp, L, Lang>>::Error>,
)
where
  L: Lexer<'inp>,
  Lang: ?Sized,
  E: Emitter<'inp, L, Lang> + ValueKeyedEmitter,
  C: Cache<'inp, L, Lang>,
  Partial: SurfaceIncomplete<'inp, L, (Sink<'inp, L, E>, C), Lang>,
  P: ParseInput<'inp, L, O, (Sink<'inp, L, E>, C), Lang, Partial>,
{
  let ctx = (Sink::new(src, inner, profile), cache);
  let mut input = Input::<L, (Sink<'inp, L, E>, C), Lang, Partial>::with_state_and_context(
    src,
    state,
    ParseContext::provide(ctx),
  );
  // The driver states the world fact BEFORE the parser ever sees a handle, exactly as
  // `parse_partial` does: `seal` takes `&mut Input`, so `f` cannot end the stream at any depth.
  if is_final {
    input.seal();
  }
  let parsed = f.parse_input(&mut input.as_ref());
  // Taken before `into_emitter` for the reason the complete driver states, and owed here for a
  // second one: a partial attempt builds a fresh input and therefore a fresh counter, so this
  // handle's count is this attempt's — a refill loop that wants the whole stream's total sums the
  // handles it kept, rather than expecting the last one to carry them.
  let resource_trips = input.resource_trips();
  (Cst::from_sink(input.into_emitter(), resource_trips), parsed)
}
