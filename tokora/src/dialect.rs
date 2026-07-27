use crate::{
  ErrorOf, Lexer, SliceOf,
  input::{Complete, InputRef},
};

/// The two facts a grammar cannot derive: which lexer it runs on, and which language it brands.
///
/// Every generic production in a real grammar needs the same block of equalities — the lexer's
/// source, slice, token, span and offset, plus the token capabilities it classifies through —
/// and Rust has no way to name that block once. Written out, it is a dozen `where`-clause lines
/// per function, copied into every one of them; a grammar of a few hundred productions carries
/// a few hundred copies. `Dialect` is the anchor those equalities hang off: one type per
/// language names its lexer and its brand, a one-line subtrait pins everything reachable from
/// the lexer, and each production then carries two `where`-clause lines instead of sixteen.
///
/// # Why exactly two associated items
///
/// `Dialect` is an *anchor*, not a configuration object. Source, slice, token, span, offset and
/// every token capability are already reachable by projection from [`Lexer`], so adding them
/// here would only mean one more line every dialect must spell, with no new expressive power.
/// A dialect that wants to *pin* those projections does it in a subtrait, where the pin is
/// written once and inherited by every production — see the example below.
///
/// # Examples
///
/// The whole pattern: one anchor impl, one subtrait carrying the equality block, and a
/// production whose `where` clause is two lines and restates nothing about the error type,
/// the emitter, the span or the offset.
///
/// ```rust
/// use core::marker::PhantomData;
///
/// use tokora::{
///   ComposableParseContext, Lexer, SimpleSpan,
///   dialect::{Dialect, DialectErrorOf, DialectInput, LexerOf},
/// };
///
/// /// The language brand. A one-token marker; it is never instantiated.
/// struct MyLang;
///
/// /// The anchor. One impl per dialect, and this is the only place the lexer is named.
/// struct MyDialect<L>(PhantomData<L>);
///
/// impl<'inp, L: Lexer<'inp>> Dialect<'inp> for MyDialect<L> {
///   type Lang = MyLang;
///   type Lexer = L;
/// }
///
/// /// The equality block, stated **once**. Everything a production needs to know about the
/// /// lexer beyond "it is a lexer" is pinned here.
/// trait MyDialectBound<'inp>:
///   Dialect<'inp, Lang = MyLang, Lexer: Lexer<'inp, Span = SimpleSpan, Offset = usize>>
/// {
/// }
///
/// impl<'inp, D> MyDialectBound<'inp> for D where
///   D: Dialect<'inp, Lang = MyLang, Lexer: Lexer<'inp, Span = SimpleSpan, Offset = usize>>
/// {
/// }
///
/// /// …and every production is two `where`-clause lines. The annotation on `end` holds because
/// /// `Offset = usize` elaborated to here, and `SimpleSpan` is the declared return type because
/// /// `Span = SimpleSpan` did — neither is restated.
/// fn production<'inp, D, Ctx>(
///   inp: &mut DialectInput<'inp, '_, D, Ctx>,
/// ) -> Result<SimpleSpan, DialectErrorOf<'inp, D, Ctx>>
/// where
///   D: MyDialectBound<'inp>,
///   Ctx: ComposableParseContext<'inp, LexerOf<'inp, D>, MyLang>,
/// {
///   let end: usize = *inp.offset();
///   Ok(SimpleSpan::new(0, end))
/// }
/// ```
///
/// # Forgetting the impl, and one thing to know about subtraits
///
/// `Dialect` is user-implemented and never blanket-implemented, so a missing impl *is* the root
/// unsatisfied obligation and the curated message below fires directly on it — including when
/// the dialect is only reached through [`LexerOf`], [`LangOf`] or one of the input aliases.
///
/// It does **not** fire through a subtrait like `MyDialectBound` above, and that is worth
/// knowing rather than discovering: rustc reports the *root* obligation and attaches that
/// trait's attribute, and a blanket-implemented subtrait's root is the subtrait, so the message
/// becomes rustc's default `the trait bound X: MyDialectBound<'_> is not satisfied` with
/// `Dialect` mentioned only in the `help:` line. A consumer who wants a curated message at
/// their own subtrait should put a `#[diagnostic::on_unimplemented]` on it — the attribute is
/// theirs to write, and tokora cannot write it for them.
///
/// # There is no `DialectCtx`, and there cannot be
///
/// The obvious next convenience is a second subtrait that hides the context bound too:
///
/// ```rust,ignore
/// trait DialectCtx<'inp, D>: ComposableParseContext<'inp, LexerOf<'inp, D>, LangOf<'inp, D>>
/// where D: Dialect<'inp> {}
/// ```
///
/// so a production could write `Ctx: DialectCtx<'inp, D>` and drop the brand from its
/// signature. **It does not work, and it is not shipped.** Routing the language through
/// [`LangOf`] leaves the bound unable to discharge
/// [`InputRef`]'s own `Ctx: ParseContext<'inp, L, Lang>` obligation: rustc
/// does not normalise a projection inside an elaborated supertrait predicate, so
/// `ComposableParseContext<'inp, _, LangOf<'inp, D>>` and
/// `ParseContext<'inp, _, <D as Dialect<'inp>>::Lang>` are not seen as the same obligation.
/// The brand has to be named literally at the bound.
///
/// This costs nothing at the use site — the brand is the consumer's own one-token marker, and
/// it is one token — so the constraint is recorded rather than worked around. The next two
/// examples are the record: they differ in exactly one token, and only the one naming the
/// brand literally compiles.
///
/// Routed through [`LangOf`] — **does not compile**:
///
/// ```rust,compile_fail,E0277
/// use tokora::{
///   ComposableParseContext,
///   dialect::{Dialect, DialectInput, LangOf, LexerOf},
/// };
///
/// struct MyLang;
///
/// fn production<'inp, D, Ctx>(inp: &mut DialectInput<'inp, '_, D, Ctx>)
/// where
///   D: Dialect<'inp, Lang = MyLang>,
///   Ctx: ComposableParseContext<'inp, LexerOf<'inp, D>, LangOf<'inp, D>>,
/// {
///   let _ = inp;
/// }
/// ```
///
/// The same signature with the brand named literally — compiles:
///
/// ```rust
/// use tokora::{
///   ComposableParseContext,
///   dialect::{Dialect, DialectInput, LexerOf},
/// };
///
/// struct MyLang;
///
/// fn production<'inp, D, Ctx>(inp: &mut DialectInput<'inp, '_, D, Ctx>)
/// where
///   D: Dialect<'inp, Lang = MyLang>,
///   Ctx: ComposableParseContext<'inp, LexerOf<'inp, D>, MyLang>,
/// {
///   let _ = inp;
/// }
/// ```
///
/// The pair is also the tripwire for the day the constraint lifts: if rustc learns to
/// normalise that projection, the first example starts compiling and its `compile_fail` fails.
/// That is the signal to revisit `DialectCtx` — not a defect in the test.
#[diagnostic::on_unimplemented(
  message = "`{Self}` is not a tokora dialect",
  label = "this type has no `Dialect` impl, so its lexer and language brand are unknown",
  note = "a dialect is one impl naming the two things a production cannot derive: \
          `impl<'inp, Src> Dialect<'inp> for MyDialect<Src> \
          {{ type Lang = MyLang; type Lexer = MyLexer<'inp, Src>; }}`",
  note = "everything else — source, slice, token, span, offset, token capabilities — is \
          reachable by projection from `Lexer`, so pin those in a one-line subtrait \
          (`trait MyDialectBound<'inp>: Dialect<'inp, Lang = MyLang, Lexer: Lexer<'inp, …>> {{}}`) \
          rather than as further associated items"
)]
pub trait Dialect<'inp> {
  /// The language brand this dialect stamps on spans, punctuators and errors.
  ///
  /// It is a marker, never a value: a one-token type whose only job is to keep two grammars'
  /// vocabularies from unifying. `?Sized` so a brand may be an extern type or a trait object
  /// if a consumer wants one.
  type Lang: ?Sized;

  /// The lexer this dialect's productions run on.
  ///
  /// Source, slice, token, span and offset all project from here, which is why they are not
  /// associated items of their own.
  type Lexer: Lexer<'inp>;
}

/// The lexer a dialect runs on.
///
/// Alias for `<D as Dialect<'inp>>::Lexer`.
pub type LexerOf<'inp, D> = <D as Dialect<'inp>>::Lexer;

/// The language brand a dialect stamps.
///
/// Alias for `<D as Dialect<'inp>>::Lang`.
///
/// Useful for naming the brand in a return type or an alias. It is **not** usable in a context
/// bound — see [`Dialect`]'s "There is no `DialectCtx`" section for the measurement and the
/// reason.
pub type LangOf<'inp, D> = <D as Dialect<'inp>>::Lang;

/// The source slice a dialect's lexer yields — what its AST nodes are generic over.
///
/// Alias for [`SliceOf`]`<'inp, `[`LexerOf`]`<'inp, D>>`.
pub type DialectSlice<'inp, D> = SliceOf<'inp, LexerOf<'inp, D>>;

/// The input handle a dialect's productions take.
///
/// Alias for [`InputRef`] with the dialect's lexer and brand already
/// substituted, so a production's parameter is `&mut DialectInput<'inp, '_, D, Ctx>` rather
/// than a five-parameter spelling.
pub type DialectInput<'inp, 'c, D, Ctx, Cmpl = Complete> =
  InputRef<'inp, 'c, LexerOf<'inp, D>, Ctx, LangOf<'inp, D>, Cmpl>;

/// The error type a dialect's productions return.
///
/// Alias for [`ErrorOf`]`<'inp, `[`LexerOf`]`<'inp, D>, Ctx, `[`LangOf`]`<'inp, D>>` — the
/// emitter's error, reached through the context.
pub type DialectErrorOf<'inp, D, Ctx> = ErrorOf<'inp, LexerOf<'inp, D>, Ctx, LangOf<'inp, D>>;
