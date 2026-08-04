use core::mem::MaybeUninit;

use generic_arraydeque::{ArrayLength, GenericArrayDeque, array::GenericArray, typenum};

use crate::{
  cache::Peeked,
  emitter::EmitterView,
  input::{DelimClass, InputRef},
  located::Located,
  parser::*,
  punct::*,
  slice::Sliced,
  span::Spanned,
  token::PunctuatorToken,
  utils::marker::{PhantomLocated, PhantomSliced, PhantomSpan},
};

use super::*;

mod sealed {
  pub trait Sealed {}
}

/// A trait for parsers that specify the capacity of their peek buffer.
///
/// Peek windows are capped at `U32` (32 tokens): `Window` is implemented for
/// `typenum::U1` through `typenum::U32` only.
pub trait Window: sealed::Sealed {
  /// The capacity of the peek buffer.
  type CAPACITY: ArrayLength;

  /// Create an uninitialized array of the specified capacity.
  #[inline(always)]
  fn array<T>() -> GenericArray<MaybeUninit<T>, Self::CAPACITY> {
    GenericArray::uninit()
  }

  /// Create a deque of the specified capacity.
  #[inline(always)]
  fn deque<T>() -> GenericArrayDeque<MaybeUninit<T>, Self::CAPACITY> {
    GenericArrayDeque::new()
  }
}

macro_rules! peek_buf_capacity_impl_for_typenum {
  ($($size:literal), + $(,)?) => {
    paste::paste! {
      $(
        impl sealed::Sealed for typenum::[< U $size >] {}

        impl Window for typenum::[< U $size >] {
          type CAPACITY = typenum::[< U $size >];
        }
      )*
    }
  };
}

// Peek windows are capped at `U32`: only `typenum::U1..=U32` receive a `Window` impl.
seq_macro::seq!(N in 1..=32 {
  peek_buf_capacity_impl_for_typenum! {
    #(N,)*
  }
});

/// Decision action for conditional parsing.
///
/// # The emitter's operations, not the emitter
///
/// The second parameter is an [`EmitterView`] — the emitter's own methods under their own names,
/// in a value that implements no emitter trait. It used to be `&mut E`, and `&mut E` **is** an
/// emitter: a condition handed one could wrap it and install the wrapper as the context of a
/// second parse over a **different** buffer, so a recording sink pinned to one source recorded a
/// foreign parse's structure and materialized it over its own bytes. A view has nothing to install.
///
/// Everything a condition could do to the emitter it can still do to the view — the whole
/// diagnostic surface, the CST structuring surface, and every capability channel — so a condition
/// still decides *and reports* inline, with no rewind. See [`EmitterView`] for the four members
/// deliberately withheld (all four are the input layer's own bookkeeping, and none was ever
/// callable from a condition without desynchronizing the parse).
pub trait Decision<'inp, L, E, W, Lang: ?Sized = ()> {
  /// Decide the next action based on the peeked tokens.
  fn decide(
    &mut self,
    toks: Peeked<'_, 'inp, L, W>,
    emitter: EmitterView<'_, 'inp, L, E, Lang>,
  ) -> Result<Action, E::Error>
  where
    L: Lexer<'inp>,
    E: Emitter<'inp, L, Lang>,
    W: Window;
}

impl<'inp, F, L, E, W, Lang: ?Sized> Decision<'inp, L, E, W, Lang> for F
where
  F: FnMut(Peeked<'_, 'inp, L, W>, EmitterView<'_, 'inp, L, E, Lang>) -> Result<Action, E::Error>,
  L: Lexer<'inp>,
  E: Emitter<'inp, L, Lang>,
  W: Window,
{
  #[inline(always)]
  fn decide(
    &mut self,
    toks: Peeked<'_, 'inp, L, W>,
    emitter: EmitterView<'_, 'inp, L, E, Lang>,
  ) -> Result<Action, E::Error>
  where
    W: Window,
  {
    (self)(toks, emitter)
  }
}

/// Width-1 decision adapters in grammar vocabulary.
///
/// Each constructor returns a concrete type implementing [`Decision`] **pinned at
/// `W = U1`**, which is what kills the turbofish: the driver's window parameter infers
/// from the adapter, so the call site carries no `::<_, U1>` and no [`Peeked`] in the
/// hook. A `W`-generic impl re-ambiguates it.
///
/// A second blanket closure impl would break coherence with the `FnMut` blanket above,
/// so concrete adapter types are the house pattern here — see `WhileNext` in
/// `parser/list.rs`.
mod decision_adapters {
  use super::*;

  /// Continue while the head satisfies `pred`; stop at a failing head or end of input.
  pub struct WhileHead<F>(pub(super) F);
  /// Continue while the head's kind equals `kind` — the punct-tail idiom.
  pub struct WhileKind<K>(pub(super) K);

  impl<F> core::fmt::Debug for WhileHead<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("WhileHead").finish_non_exhaustive()
    }
  }

  impl<K: core::fmt::Debug> core::fmt::Debug for WhileKind<K> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("WhileKind").field("kind", &self.0).finish()
    }
  }

  impl<'inp, F, L, E, Lang: ?Sized> Decision<'inp, L, E, typenum::U1, Lang> for WhileHead<F>
  where
    F: FnMut(&L::Token) -> bool,
    L: Lexer<'inp>,
    E: Emitter<'inp, L, Lang>,
  {
    #[inline(always)]
    fn decide(
      &mut self,
      mut toks: Peeked<'_, 'inp, L, typenum::U1>,
      _emitter: EmitterView<'_, 'inp, L, E, Lang>,
    ) -> Result<Action, E::Error> {
      use crate::cache::PeekedTokenExt as _;
      Ok(match toks.pop_front() {
        Some(t) if (self.0)(t.token()) => Action::Continue,
        _ => Action::Stop,
      })
    }
  }

  impl<'inp, K, L, E, Lang: ?Sized> Decision<'inp, L, E, typenum::U1, Lang> for WhileKind<K>
  where
    K: PartialEq<<L::Token as Token<'inp>>::Kind>,
    L: Lexer<'inp>,
    E: Emitter<'inp, L, Lang>,
  {
    #[inline(always)]
    fn decide(
      &mut self,
      mut toks: Peeked<'_, 'inp, L, typenum::U1>,
      _emitter: EmitterView<'_, 'inp, L, E, Lang>,
    ) -> Result<Action, E::Error> {
      use crate::cache::PeekedTokenExt as _;
      Ok(match toks.pop_front() {
        Some(t) if self.0 == t.token().kind() => Action::Continue,
        _ => Action::Stop,
      })
    }
  }
}
pub use decision_adapters::{WhileHead, WhileKind};

/// Continue while the head satisfies `pred` — width-1, no turbofish, no [`Peeked`].
///
/// The closure takes the head by reference. Bare-closure parameters do not infer through
/// an impl-side `Fn` bound, so a closure written inline needs a `|t: &Tok|` ascription; a
/// named function needs none. [`while_kind`] needs neither.
///
/// "Until" is the same adapter with one `!`: `while_head(|t: &Tok| !t.is_semicolon())`.
#[inline(always)]
pub fn while_head<F>(pred: F) -> WhileHead<F> {
  decision_adapters::WhileHead(pred)
}

/// Continue while the head's kind equals `kind` — zero annotation at the call site.
#[inline(always)]
pub fn while_kind<K>(kind: K) -> WhileKind<K> {
  decision_adapters::WhileKind(kind)
}

/// A parser adapter that pins the enclosed parser's output type to `O`.
///
/// A parser with several output impls — `Collect`, `With`, `FailWith`, `Accepted` — is
/// ambiguous at every downstream site that does not name the output. `Pinned<P, O>`
/// implements [`ParseInput`]/[`TryParseInput`](crate::TryParseInput) at exactly one `O`,
/// so the ambiguity stops there instead of being re-annotated at every use.
///
/// Build one with [`pinned`].
pub struct Pinned<P, O> {
  inner: P,
  // `fn() -> O`, not `O`: the adapter stores no `O`, so it must not inherit `O`'s
  // auto-traits — `pinned::<*const u8, _>(p)` must not un-`Send` a `Send` parser. A
  // fn-pointer phantom is `Send + Sync` unconditionally and keeps covariance in `O`.
  // Variance and auto-traits are public API the moment they ship.
  _pin: core::marker::PhantomData<fn() -> O>,
}

impl<P: core::fmt::Debug, O> core::fmt::Debug for Pinned<P, O> {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("Pinned")
      .field("inner", &self.inner)
      .finish()
  }
}

/// Pins `parser`'s output type to `O2`, so a receiver with several output impls stops
/// being ambiguous downstream.
///
/// ```ignore
/// let items = pinned::<Vec<i64>, _>(element.repeated_while(cond).collect())
///   .parse_input(inp)?;
/// let n = items.len(); // resolves: the output type is no longer a variable
/// ```
///
/// # Why this is a free function and not a method
///
/// The fluent spelling would have to be a **blanket** trait method — bounding it by
/// [`ParseInput`] means naming the very parameters whose ambiguity it exists to kill, and
/// a `Self`-returning method on `ParseInput` re-ambiguates (the next call sees fresh
/// inference variables). A blanket method plants a candidate on **every `Sized` type** in
/// the consumer's program, and a by-value candidate is picked before a consumer's
/// same-named `&self` method.
///
/// That was accepted while it was believed such a collision must be loud, because `O2` is
/// not determined by the receiver. It is not: a caller can supply `O2` with a turbofish,
/// and if the return value is discarded the call compiles and the consumer's method
/// silently stops running — with no error and no warning. Worse, an extension method that
/// exists to *pin a type* has no inferred spelling at all, so the turbofish is its only
/// spelling and the silent path is the default rather than a corner.
///
/// A free function resolves by path, plants nothing in method space, and cannot shadow
/// anything. The fluent form remains purely additive later if the hazard is ever
/// understood well enough to price it.
#[inline(always)]
pub fn pinned<O2, P>(parser: P) -> Pinned<P, O2> {
  Pinned {
    inner: parser,
    _pin: core::marker::PhantomData,
  }
}

impl<'inp, L, O, Ctx, Lang: ?Sized, Cmpl, P> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for Pinned<P, O>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness,
  {
    self.inner.parse_input(input)
  }
}

impl<'inp, L, O, Ctx, Lang: ?Sized, Cmpl, P> crate::TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for Pinned<P, O>
where
  P: crate::TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
{
  #[inline(always)]
  fn try_parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    crate::try_parse_input::ParseAttempt<O>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness,
  {
    self.inner.try_parse_input(input)
  }
}

/// A trait for parsers that accumulate their results into a container.
pub trait Accumulator<'inp, L, Container, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  /// Collects the parsed elements into the specified container.
  #[inline(always)]
  fn collect(self) -> Collect<Self, Container, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    Container: Default,
    Collect<Self, Container, Ctx, Lang, Cmpl>: ParseInput<'inp, L, Container, Ctx, Lang, Cmpl>,
  {
    Collect::new(self, Container::default())
  }

  /// Collects the parsed elements with the given container.
  #[inline(always)]
  fn collect_with(self, container: Container) -> Collect<Self, Container, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    Collect<Self, Container, Ctx, Lang, Cmpl>: ParseInput<'inp, L, Container, Ctx, Lang, Cmpl>,
  {
    Collect::new(self, container)
  }
}

impl<'inp, P, Container, L, Ctx, Lang: ?Sized, Cmpl>
  Accumulator<'inp, L, Container, Ctx, Lang, Cmpl> for P
where
  Collect<P, Container, Ctx, Lang, Cmpl>: ParseInput<'inp, L, Container, Ctx, Lang, Cmpl>,
{
}

macro_rules! define_separated_by {
  ($($name:ident),+$(,)?) => {
    paste::paste! {
      $(
        #[doc = "Creates a `SeparatedWhile` combinator which separates elements by the `" $name:snake "` separator and applies this parser repeatedly."]
        ///
        /// See [`separated_while`](crate::ParseInput::separated_while) for details.
        #[inline(always)]
        fn [< separated_by_ $name:snake _while>]<Condition, W>(
          self,
          condition: Condition,
        ) -> SeparatedWhile<Self, $name<(), (), Lang>, Condition, O, W, L, Ctx, Lang, Cmpl>
        where
          Self: Sized,
          L: Lexer<'inp>,
          L::Token: PunctuatorToken<'inp>,
          Ctx: ParseContext<'inp, L, Lang>,
          Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
          W: Window,
        {
          SeparatedWhile::new::<$name<(), (), Lang>>(self, condition)
        }
      )*
    }
  };
}

macro_rules! define_delimited_by {
  ($($method:ident => $delimiter:ident),+$(,)?) => {
    $(
      #[doc = concat!(
        "Wraps this parser in a committed `",
        stringify!($delimiter),
        "` delimiter pair by delegating to `ParseInput::delimited`."
      )]
      #[inline(always)]
      fn $method(
        self,
      ) -> impl for<'c> FnMut(
        &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
      ) -> crate::parser::DelimitedOf<'inp, $delimiter<(), (), Lang>, L, Ctx, Lang, O>
      where
        Self: Sized,
        $delimiter<(), (), Lang>: crate::delimiter::TypedDelimiter<'inp, L, Lang>,
        L: Lexer<'inp>,
        Ctx: ComposableParseContext<'inp, L, Lang>,
        Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
      {
        self.delimited::<$delimiter<(), (), Lang>>()
      }
    )+
  };
}

macro_rules! define_try_delimited_by {
  ($($method:ident => $delimiter:ident),+$(,)?) => {
    $(
      #[doc = concat!(
        "Wraps this parser in an attempted `",
        stringify!($delimiter),
        "` delimiter pair by delegating to `ParseInput::try_delimited`."
      )]
      ///
      /// Delegation is the contract, not an implementation detail: this method is the
      /// fluent form of the free function and holds no state machine of its own, so the
      /// two surfaces cannot drift apart.
      #[inline(always)]
      fn $method(
        self,
      ) -> impl for<'c> FnMut(
        &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
      ) -> crate::parser::TryDelimitedOf<'inp, $delimiter<(), (), Lang>, L, Ctx, Lang, O>
      where
        Self: Sized,
        $delimiter<(), (), Lang>: crate::delimiter::TypedDelimiter<'inp, L, Lang>,
        L: Lexer<'inp>,
        Ctx: ComposableParseContext<'inp, L, Lang>,
        Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
      {
        self.try_delimited::<$delimiter<(), (), Lang>>()
      }
    )+
  };
}

/// Core trait implemented by every parser combinator.
///
/// This mirrors the ergonomics of libraries like `winnow`: a parser is
/// simply something that can mutate an [`InputRef`] and either produce
/// a value or a spanned error using the configured `Emitter`.
///
/// Since 0.3.0 the trait carries the input's [`Completeness`] typestate as its trailing
/// `Cmpl` parameter (defaulted to [`Complete`], so every pre-0.3.0 spelling reads
/// unchanged). A parser written generic over `Cmpl` is a **write-once** parser: the same
/// item runs under the complete drivers ([`Parse`](crate::Parse)) and the Sans-I/O partial
/// driver ([`parse_partial`](crate::parse_partial)). Builder methods return adapters that
/// carry the same parameter, so a whole combinator chain stays mode-generic until a drive
/// site pins it.
pub trait ParseInput<'inp, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  /// Try to parse from the given input.
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;

  /// Wraps this parser in a committed delimiter pair.
  ///
  /// This is the method form of [`delimited`]. The delimiter type
  /// selects the opener and closer, while this parser produces the enclosed data.
  #[inline(always)]
  fn delimited<D>(
    self,
  ) -> impl for<'c> FnMut(
    &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
  ) -> crate::parser::DelimitedOf<'inp, D, L, Ctx, Lang, O>
  where
    Self: Sized,
    D: crate::delimiter::TypedDelimiter<'inp, L, Lang>,
    L: Lexer<'inp>,
    Ctx: ComposableParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  {
    crate::parser::delimited::<D, L, Ctx, Lang, Self, O, Cmpl>(self)
  }

  define_delimited_by! {
    delimited_by_parens => Paren,
    delimited_by_braces => Brace,
    delimited_by_brackets => Bracket,
    delimited_by_angles => Angle,
  }

  /// Wraps this parser in an attempted delimiter pair.
  ///
  /// This is the method form of [`try_delimited`], the attempt twin of
  /// [`delimited`](Self::delimited): only the **opener** is tentative. The attempt declines
  /// — `Ok(None)`, zero consumption — when the `D` opener is definitely absent or the input
  /// genuinely ended; a terminal scanner stop is not a decline. Once the opener is consumed
  /// the parse is committed, so this parser's errors and the missing-closer diagnostics
  /// behave exactly as the committed form's do.
  ///
  /// Delegation is the contract, not an implementation detail: the body is
  /// `crate::parser::try_delimited(self)` and nothing else, so the free function and this
  /// method cannot drift apart.
  ///
  /// The free form remains the escape hatch when the enclosed output type has to be named in
  /// a turbofish: `O` is a parameter of this trait rather than of the method, so it can only
  /// be pinned here by annotating the enclosed parser or its container.
  #[inline(always)]
  fn try_delimited<D>(
    self,
  ) -> impl for<'c> FnMut(
    &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
  ) -> crate::parser::TryDelimitedOf<'inp, D, L, Ctx, Lang, O>
  where
    Self: Sized,
    D: crate::delimiter::TypedDelimiter<'inp, L, Lang>,
    L: Lexer<'inp>,
    Ctx: ComposableParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  {
    crate::parser::try_delimited::<D, L, Ctx, Lang, Self, O, Cmpl>(self)
  }

  define_try_delimited_by! {
    try_delimited_by_parens => Paren,
    try_delimited_by_braces => Brace,
    try_delimited_by_brackets => Bracket,
    try_delimited_by_angles => Angle,
  }

  /// Wraps the output of this parser in a `Spanned` with the span of the parsed input.
  #[inline(always)]
  fn spanned(self) -> With<PhantomSpan, Self, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
  {
    With::new(PhantomSpan::phantom(), self)
  }

  /// Wraps the output of this parser in a `Sliced` with the source slice of the parsed input.
  #[inline(always)]
  fn sliced(self) -> With<PhantomSliced, Self, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
  {
    With::new(PhantomSliced::phantom(), self)
  }

  /// Wraps the output of this parser in a `Located` with the span and source slice of the parsed input.
  #[inline(always)]
  fn located(self) -> With<PhantomLocated, Self, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
  {
    With::new(PhantomLocated::phantom(), self)
  }

  /// Ignores the output of this parser.
  #[inline(always)]
  fn ignored(self) -> Ignore<Self, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ignore<Self, O, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, (), Ctx, Lang, Cmpl>,
  {
    Ignore::new(self)
  }

  /// Creates a parser over a mutable reference to this parser.
  #[inline(always)]
  fn by_ref(&mut self) -> &mut ByRef<Self> {
    ByRef::from_ref_mut(self)
  }

  /// Creates a `FoldWhile` combinator that accumulates results while a condition is met.
  #[inline(always)]
  fn fold_while<Condition, Init, Acc, W>(
    self,
    pred: Condition,
    init: Init,
    acc: Acc,
  ) -> FoldWhile<Self, Condition, Init, Acc, O, W, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
    FoldWhile<Self, Condition, Init, Acc, O, W, L, Ctx, Lang, Cmpl>:
      ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    FoldWhile::new(self, pred, init, acc)
  }

  /// Creates a `TryFoldWhile` combinator that accumulates results while a condition is met.
  ///
  /// See also [`try_fold_while_with`](Self::try_fold_while_with).
  #[inline(always)]
  fn try_fold_while<Condition, Init, Acc, W>(
    self,
    pred: Condition,
    init: Init,
    acc: Acc,
  ) -> TryFoldWhile<Self, Condition, Init, Acc, O, W, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Init: FnMut() -> O,
    Acc: FnMut(O, O) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
    TryFoldWhile<Self, Condition, Init, Acc, O, W, L, Ctx, Lang, Cmpl>:
      ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    TryFoldWhile::new(self, pred, init, acc)
  }

  /// Creates a `TryFoldWhileWith` combinator that accumulates results while a condition is met,
  /// with access to parsing state.
  #[inline(always)]
  fn try_fold_while_with<Condition, Init, Acc, W>(
    self,
    pred: Condition,
    init: Init,
    acc: Acc,
  ) -> TryFoldWhileWith<Self, Condition, Init, Acc, O, W, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Init: FnMut() -> O,
    Acc: FnMut(
      O,
      O,
      ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
    ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
    TryFoldWhileWith<Self, Condition, Init, Acc, O, W, L, Ctx, Lang, Cmpl>:
      ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    TryFoldWhileWith::new(self, pred, init, acc)
  }

  /// Creates a `RFoldWhile` combinator that applies this parser repeatedly,
  /// while a condition is met, and folds results in reverse order.
  ///
  /// This buffers all parsed outputs before folding them from right to left.
  ///
  /// See also [`fold_while`](Self::fold_while).
  #[cfg(any(feature = "alloc", feature = "std"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
  #[inline(always)]
  fn rfold_while<Condition, Init, Acc, W>(
    self,
    condition: Condition,
    init: Init,
    acc: Acc,
  ) -> RFoldWhile<Self, Condition, Init, Acc, L, O, W, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Init: FnMut() -> O,
    Acc: FnMut(O, O) -> O,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
    RFoldWhile<Self, Condition, Init, Acc, L, O, W, Ctx, Lang, Cmpl>:
      ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    RFoldWhile::new(self, condition, init, acc)
  }

  /// Creates a `RepeatedWhile` combinator that applies this parser repeatedly, where **you
  /// provide the lookahead logic**.
  ///
  /// The parser will be called repeatedly until:
  /// - Your condition function returns `Action::Stop` - you decided to stop based on lookahead
  /// - It returns `Err(e)` - fatal error
  ///
  /// ## Key Behavior
  ///
  /// Unlike [`repeated()`](TryParseInput::repeated), this parser doesn't need built-in lookahead:
  /// - **You provide** a condition function that peeks ahead at tokens
  /// - Condition decides `Continue` or `Stop` based on what it sees
  /// - Element parser is only called when condition says `Continue`
  ///
  /// ## Type Parameters
  ///
  /// - `W`: Window size for lookahead (e.g., `U1` for 1 token, `U2` for 2 tokens)
  ///
  /// ## See Also
  ///
  /// - [`repeated`](TryParseInput::repeated) - Parser has lookahead, no separator
  /// - [`Action`] - The decision type (`Continue` or `Stop`)
  #[inline(always)]
  fn repeated_while<Condition, W>(
    self,
    condition: Condition,
  ) -> RepeatedWhile<Self, Condition, O, W, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
  {
    RepeatedWhile::new(self, condition)
  }

  /// Creates a `SeparatedWhile` combinator that parses separated elements, where **you
  /// provide the lookahead logic**.
  ///
  /// The parser will be called repeatedly to parse elements separated by the given separator,
  /// until:
  /// - Your condition function returns `Action::Stop` - you decided to stop based on lookahead
  /// - It returns `Err(e)` - fatal error
  ///
  /// ## Key Behavior
  ///
  /// Unlike [`separated()`](TryParseInput::separated), this parser doesn't need built-in lookahead:
  /// - **You provide** a condition function that peeks ahead at tokens
  /// - Condition decides `Continue` or `Stop` based on what it sees
  /// - Element parser is only called when condition says `Continue`
  /// - Separator is parsed between elements
  ///
  /// ## Type Parameters
  ///
  /// - `W`: Window size for lookahead (e.g., `U1` for 1 token, `U2` for 2 tokens)
  ///
  /// ## See Also
  ///
  /// - [`separated`](TryParseInput::separated) - Parser has lookahead, with separator
  /// - [`Action`] - The decision type (`Continue` or `Stop`)
  #[inline(always)]
  fn separated_while<Sep, Condition, W>(
    self,
    condition: Condition,
  ) -> SeparatedWhile<Self, Sep, Condition, O, W, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    Sep: Punctuator<'inp, L, Lang>,
    W: Window,
  {
    SeparatedWhile::new::<Sep>(self, condition)
  }

  define_separated_by!(
    Comma,
    Semicolon,
    Dot,
    Colon,
    Pipe,
    Ampersand,
    Hyphen,
    Underscore,
    DoubleColon,
    Arrow,
    FatArrow,
    Tilde,
    Slash,
    Backslash,
    Percent,
    Dollar,
    Hash,
    At,
  );

  /// Method form of the free [`labelled`](crate::parser::labelled).
  #[inline(always)]
  fn labelled(self, name: &'static str) -> crate::parser::Labelled<Self>
  where
    Self: Sized,
  {
    crate::parser::labelled(name, self)
  }

  /// Method form of the free [`traced`](crate::trace::traced).
  #[cfg(feature = "trace")]
  #[cfg_attr(docsrs, doc(cfg(feature = "trace")))]
  #[inline(always)]
  fn traced(self, name: &'static str) -> crate::trace::Traced<Self>
  where
    Self: Sized,
  {
    crate::trace::traced(name, self)
  }

  /// Method form of the free [`traced`](crate::trace::traced).
  ///
  /// With `trace` off this is the identity, exactly as the free function is.
  #[cfg(not(feature = "trace"))]
  #[cfg_attr(docsrs, doc(cfg(not(feature = "trace"))))]
  #[inline(always)]
  fn traced(self, _name: &'static str) -> Self
  where
    Self: Sized,
  {
    self
  }

  /// Method form of the free [`list`](crate::parser::list) — named `list_until` because
  /// the `until` argument is the distinguishing half.
  ///
  /// # Availability
  ///
  /// Complete inputs only ([`Complete`](crate::input::Complete)), and the context must be
  /// a [`ComposableParseContext`]. The completeness pin is **inherited from the free
  /// [`list`](crate::parser::list)**, which carries it already: this method cannot be
  /// wider than the function it delegates to, and widening that function would re-bound a
  /// pre-existing item, which this wave does not do. Streaming callers keep the
  /// element-level primitives. (Contrast [`select!`](crate::select), whose runtime is new
  /// here and is therefore generic over completeness.)
  ///
  /// The `alloc`/`std` gate is the free function's too — `list` returns a `Vec`.
  #[cfg(any(feature = "alloc", feature = "std"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
  #[inline(always)]
  fn list_until<Until>(
    self,
    until: Until,
  ) -> impl for<'c> FnMut(
    &mut InputRef<'inp, 'c, L, Ctx, Lang>,
  ) -> crate::parser::ListOf<'inp, L, Ctx, Lang, O>
  where
    Self: Sized + ParseInput<'inp, L, O, Ctx, Lang>,
    L: Lexer<'inp>,
    Ctx: ComposableParseContext<'inp, L, Lang>,
    Until: FnMut(&L::Token) -> bool,
  {
    crate::parser::list(self, until)
  }

  /// Method form of the free [`separated1`](crate::parser::separated1) — the
  /// committed-first "light" list shape, fluent.
  ///
  /// # Availability
  ///
  /// Same as [`list_until`](Self::list_until): complete inputs only, and a
  /// [`ComposableParseContext`]. The pin is inherited from the free
  /// [`separated1`](crate::parser::separated1), and so is the `alloc`/`std` gate — the
  /// free function returns a `Vec`.
  #[cfg(any(feature = "alloc", feature = "std"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
  #[inline(always)]
  fn separated1_by<Sep, Peek>(
    self,
    peek: Peek,
  ) -> impl for<'c> FnMut(
    &mut InputRef<'inp, 'c, L, Ctx, Lang>,
  ) -> crate::parser::Separated1Of<'inp, L, Ctx, Lang, O>
  where
    Self: Sized + ParseInput<'inp, L, O, Ctx, Lang>,
    L: Lexer<'inp>,
    Ctx: ComposableParseContext<'inp, L, Lang>,
    Sep: Punctuator<'inp, L, Lang>,
    Peek: FnMut(&L::Token) -> bool,
  {
    crate::parser::separated1::<Sep, L, Ctx, Lang, Self, O, Peek>(self, peek)
  }

  /// Creates a `PeekThen` combinator that peeks at most `N` tokens first from the input before parsing.
  ///
  /// If the condition handler `C` returns `Ok(())`, the inner parser is applied, otherwise,
  /// parsing is stopped and return the error from the handler.
  fn peek_then<C, W>(self, condition: C) -> PeekThen<Self, C, L::Token, W, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    C: FnMut(
      Peeked<'_, 'inp, L, W>,
      EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>,
    ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    W: Window,
    PeekThen<Self, C, L::Token, W, Cmpl>: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    PeekThen::of(self, condition)
  }

  /// Creates a `PeekThen` combinator that peeks at most `N` tokens first from the input before parsing.
  ///
  /// If the condition handler `C` returns `Ok(Action::Continue)`, the inner parser is applied,
  /// otherwise returns `None`.
  #[doc(alias = "or_not")]
  fn peek_then_try<C, W>(self, condition: C) -> PeekThen<Self, C, L::Token, W, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    C: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
    PeekThen<Self, C, L::Token, W, Cmpl>: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    PeekThen::of(self, condition)
  }

  /// The width-1 twin of [`peek_then`](Self::peek_then): the condition sees `Some(head)`
  /// or `None` (end of input) in grammar vocabulary.
  ///
  /// No [`Peeked`], no typenum, and no emitter parameter — so a hook written as a named
  /// function does not have to name `Ctx` in its signature just to be callable here.
  #[inline(always)]
  fn peek_then_head<C>(
    self,
    mut condition: C,
  ) -> PeekThen<
    Self,
    impl FnMut(
      Peeked<'_, 'inp, L, typenum::U1>,
      EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>,
    ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    L::Token,
    typenum::U1,
    Cmpl,
  >
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    C: FnMut(
      Option<Spanned<&L::Token, &L::Span>>,
    ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  {
    PeekThen::of(
      self,
      move |mut toks: Peeked<'_, 'inp, L, typenum::U1>,
            _emitter: EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>| {
        use crate::cache::PeekedTokenExt as _;
        match toks.pop_front() {
          Some(t) => condition(Some(Spanned::new(t.span(), t.token()))),
          None => condition(None),
        }
      },
    )
  }

  /// Map the output of this parser using the given function.
  #[inline(always)]
  fn map<U, F>(self, f: F) -> Map<Self, F, L, Ctx, O, U, Lang, Cmpl>
  where
    Self: Sized,
    F: FnMut(O) -> U,
    L: Lexer<'inp>,
    Map<Self, F, L, Ctx, O, U, Lang, Cmpl>: ParseInput<'inp, L, U, Ctx, Lang, Cmpl>,
  {
    Map::new(self, f)
  }

  /// Map the output of this parser using the given function.
  #[inline(always)]
  fn map_with<U, F>(self, f: F) -> MapWith<Self, F, L, Ctx, O, U, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    F: FnMut(O, ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>) -> U,
    MapWith<Self, F, L, Ctx, O, U, Lang, Cmpl>: ParseInput<'inp, L, U, Ctx, Lang, Cmpl>,
  {
    MapWith::new(self, f)
  }

  /// Filter the output of this parser using a validation function.
  #[inline(always)]
  fn filter<F>(self, validator: F) -> Filter<Self, F, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    F: FnMut(&O) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    Ctx: ParseContext<'inp, L, Lang>,
    Filter<Self, F, O, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    Filter::of(self, validator)
  }

  /// Filter the output of this parser using a validation function.
  #[inline(always)]
  fn filter_with<F>(self, validator: F) -> FilterWith<Self, F, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    F: FnMut(
      &O,
      ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
    ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    Ctx: ParseContext<'inp, L, Lang>,
    FilterWith<Self, F, O, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    FilterWith::of(self, validator)
  }

  /// Filter and map the output of this parser using a validation/transformation function.
  ///
  /// The parser must produce a `Spanned<O>` value. The mapper receives
  /// the data and span, and returns `Ok(new_value)` or an error.
  #[inline(always)]
  fn filter_map<U, F>(self, mapper: F) -> FilterMap<Self, F, O, U, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    F: FnMut(O) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    Ctx: ParseContext<'inp, L, Lang>,
    FilterMap<Self, F, O, U, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, U, Ctx, Lang, Cmpl>,
  {
    FilterMap::of(self, mapper)
  }

  /// Filter and map the output of this parser using a validation/transformation function.
  ///
  /// The parser must produce a `Spanned<O>` value. The mapper receives
  /// the data and span, and returns `Ok(new_value)` or an error.
  #[inline(always)]
  fn filter_map_with<U, F>(self, mapper: F) -> FilterMapWith<Self, F, O, U, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    F: FnMut(
      O,
      ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
    ) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    Ctx: ParseContext<'inp, L, Lang>,
    FilterMapWith<Self, F, O, U, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, U, Ctx, Lang, Cmpl>,
  {
    FilterMapWith::of(self, mapper)
  }

  /// Validate the output of this parser with full location context.
  #[inline(always)]
  fn validate<F>(self, validator: F) -> Validate<Self, F, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    F: FnMut(&O) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    Validate::of(self, validator)
  }

  /// Validate the output of this parser with full location context.
  #[inline(always)]
  fn validate_with<F>(self, validator: F) -> ValidateWith<Self, F, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    F: FnMut(
      &O,
      ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
    ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    ValidateWith::of(self, validator)
  }

  /// Sequence this parser with another, ignoring the output of the second.
  #[inline(always)]
  fn then_ignore<G, U>(self, second: G) -> ThenIgnore<Self, G, O, U, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    G: ParseInput<'inp, L, U, Ctx, Lang, Cmpl>,
    Ctx: ParseContext<'inp, L, Lang>,
    ThenIgnore<Self, G, O, U, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    ThenIgnore::new(self, second)
  }

  /// Sequence this parser with a fixed value, ignoring the output of the first.
  #[inline(always)]
  fn then_value<F, U>(self, value: F) -> ThenValue<Self, F, O, U, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    F: FnMut() -> U,
  {
    ThenValue::new(self, value)
  }

  /// Sequence this parser with another, using the first result to determine the second parser.
  #[inline(always)]
  fn and_then<T, U>(self, then: T) -> AndThen<Self, T, O, U, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    T: FnMut(O) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    AndThen<Self, T, O, U, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, U, Ctx, Lang, Cmpl>,
  {
    AndThen::new(self, then)
  }

  /// Sequence this parser with another, using the first result to determine the second parser.
  #[inline(always)]
  fn and_then_with<T, U>(self, then: T) -> AndThenWith<Self, T, O, U, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    T: FnMut(
      O,
      ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
    ) -> Result<U, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
    Ctx: ParseContext<'inp, L, Lang>,
    L: Lexer<'inp>,
    AndThenWith<Self, T, O, U, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, U, Ctx, Lang, Cmpl>,
  {
    AndThenWith::new(self, then)
  }

  /// Sequence this parser with another, keeping both outputs.
  #[inline(always)]
  fn then<T, U>(self, then: T) -> Then<Self, T, O, U, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    T: ParseInput<'inp, L, U, Ctx, Lang, Cmpl>,
    Ctx: ParseContext<'inp, L, Lang>,
    Then<Self, T, O, U, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, (O, U), Ctx, Lang, Cmpl>,
  {
    Then::new(self, then)
  }

  /// Sequence this parser with another, ignoring the output of the first.
  #[inline(always)]
  fn ignore_then<G, U>(self, second: G) -> IgnoreThen<Self, G, O, U, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    G: ParseInput<'inp, L, U, Ctx, Lang, Cmpl>,
    IgnoreThen<Self, G, O, U, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, U, Ctx, Lang, Cmpl>,
  {
    IgnoreThen::new(self, second)
  }

  /// Recover from errors by trying an alternative parser with backtracking.
  ///
  /// If this parser fails, the input position is reset to where it was before parsing,
  /// and the recovery parser is tried from the original position. This enables trying
  /// completely different parsing strategies when the primary approach fails.
  ///
  /// # Use Cases
  ///
  /// - **Alternative interpretations**: Try parsing as different constructs
  /// - **Fallback values**: Return error/placeholder nodes on failure
  /// - **Resilient parsing**: Continue parsing to find more issues
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Parse expression or fallback to error node
  /// let parser = parse_expression()
  ///     .recover(parse_error_node());
  ///
  /// // Input: "1 + 2"      → Ok(BinaryOp(Add, 1, 2))
  /// // Input: "@ invalid" → Ok(ErrorNode(...))
  /// ```
  ///
  /// # Comparison with inplace_recover
  ///
  /// - `recover()`: Resets to starting position, tries alternative from beginning
  /// - `inplace_recover()`: Continues from error position, typically skips ahead
  ///
  /// See [`Recover`] for detailed documentation and more examples.
  #[inline(always)]
  fn recover<R>(self, recovery: R) -> Recover<Self, R, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    R: RecoverInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Ctx: ParseContext<'inp, L, Lang>,
    Recover<Self, R, O, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    Recover::new(self, recovery)
  }

  /// Recover from errors without backtracking, continuing from the error position.
  ///
  /// If this parser fails, the recovery parser starts from where the error occurred,
  /// not from the original starting position. This is typically used to skip ahead to
  /// a synchronization point (like a semicolon or brace) to resume parsing.
  ///
  /// # Use Cases
  ///
  /// - **Panic mode recovery**: Skip tokens until reaching a safe point
  /// - **Resynchronization**: Find the next statement/block boundary
  /// - **Performance**: Avoid checkpoint overhead when backtracking isn't needed
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Parse statement, skip to semicolon on error
  /// let parser = parse_statement()
  ///     .inplace_recover(
  ///         skip_to(|tok| matches!(tok, Token::Semicolon))
  ///             .then_ignore(any())
  ///             .map(|_| Statement::Error)
  ///     );
  ///
  /// // Input: "let x = 1;"     → Ok(LetStmt { .. })
  /// // Input: "bad ### ; ok"   → Ok(Statement::Error)
  /// //             ^^^ ^
  /// //        error, skip to semicolon from here
  /// ```
  ///
  /// # Comparison with recover
  ///
  /// - `recover()`: Resets to starting position, tries alternative from beginning
  /// - `inplace_recover()`: Continues from error position, typically skips ahead
  ///
  /// See [`InplaceRecover`] for detailed documentation and more examples.
  #[inline(always)]
  fn inplace_recover<R>(self, recovery: R) -> InplaceRecover<Self, R, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    R: InplaceRecoverInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Ctx: ParseContext<'inp, L, Lang>,
    InplaceRecover<Self, R, O, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    InplaceRecover::new(self, recovery)
  }

  /// Recover from errors by skipping — nesting-aware — to a synchronization point and
  /// retrying this parser.
  ///
  /// If this parser fails, the input rolls back to where the attempt began, then
  /// [`sync_balanced`](InputRef::sync_balanced) skips forward using `classifier` (which token
  /// kinds open/close delimiter pairs — see [`Balance`](crate::input::Balance)) and the
  /// depth-0 sync predicate `pred`, and the parser is retried from the sync point. Each
  /// committed skip is reported once through
  /// [`emit_skipped_region`](crate::Emitter::emit_skipped_region).
  ///
  /// A retry cycle that consumes nothing bails out with the original error (the
  /// zero-consumption progress guard), and an [`Incomplete`](crate::error::Incomplete) error
  /// is re-raised untouched without any skipping — the never-recoverable law.
  ///
  /// # Example
  ///
  /// ```ignore
  /// use tokora::input::Balance;
  ///
  /// // Parse a statement; on failure skip to the next `;` (never one inside braces) and retry.
  /// let parser = parse_statement().skip_then_retry(
  ///     |kind| match kind {
  ///         TokenKind::LBrace => Balance::Open('{'),
  ///         TokenKind::RBrace => Balance::Close('{'),
  ///         _ => Balance::Neutral,
  ///     },
  ///     |tok| matches!(tok.data(), Token::Semi),
  /// );
  /// ```
  ///
  /// See [`SkipThenRetry`] for the full loop and progress-guard contract.
  #[inline(always)]
  fn skip_then_retry<D, F>(
    self,
    classifier: D,
    pred: F,
  ) -> SkipThenRetry<Self, D, F, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    D: DelimClass<<L::Token as Token<'inp>>::Kind>,
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
    Ctx: ParseContext<'inp, L, Lang>,
    SkipThenRetry<Self, D, F, O, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    SkipThenRetry::new(self, classifier, pred)
  }

  /// Creates a parser that accepts any token with optional padding.
  #[inline(always)]
  fn padded(self) -> Padded<Self, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Padded<Self, O, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    Padded::new(self)
  }

  /// Creates a parser that accepts any token with optional padding.
  #[inline(always)]
  fn padded_left(self) -> PaddedLeft<Self, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    PaddedLeft<Self, O, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    PaddedLeft::new(self)
  }

  /// Creates a parser that accepts any token with optional padding.
  #[inline(always)]
  fn padded_right(self) -> PaddedRight<Self, O, L, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    PaddedRight<Self, O, L, Ctx, Lang, Cmpl>: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  {
    PaddedRight::new(self)
  }
}

impl<'inp, F, L, O, Ctx, Lang: ?Sized, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl> for F
where
  F: FnMut(
    &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    (self)(input)
  }
}

impl<'inp, F, L, O, Ctx, Lang: ?Sized, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for &mut ByRef<F>
where
  F: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    (**self).parse_input(input)
  }
}

impl<'inp, L, O, Ctx, P, Lang: ?Sized, Cmpl>
  ParseInput<'inp, L, Spanned<O, L::Span>, Ctx, Lang, Cmpl> for With<PhantomSpan, P, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<Spanned<O, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let cursor = inp.cursor().clone();
    self
      .secondary
      .parse_input(inp)
      .map(|output| Spanned::new(inp.span_since(&cursor), output))
  }
}

impl<'inp, L, O, Ctx, P, Lang: ?Sized, Cmpl>
  ParseInput<'inp, L, Sliced<O, <L::Source as Source<L::Offset>>::Slice<'inp>>, Ctx, Lang, Cmpl>
  for With<PhantomSliced, P, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    Sliced<O, <L::Source as Source<L::Offset>>::Slice<'inp>>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  > {
    let cursor = inp.cursor().clone();
    self.secondary.parse_input(inp).map(|output| {
      Sliced::new(
        inp
          .slice_since(&cursor)
          .expect("parser should guarantee slice"),
        output,
      )
    })
  }
}

impl<'inp, L, O, Ctx, P, Lang: ?Sized, Cmpl>
  ParseInput<
    'inp,
    L,
    Located<O, L::Span, <L::Source as Source<L::Offset>>::Slice<'inp>>,
    Ctx,
    Lang,
    Cmpl,
  > for With<PhantomLocated, P, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    Located<O, L::Span, <L::Source as Source<L::Offset>>::Slice<'inp>>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  > {
    let cursor = inp.cursor().clone();
    self.secondary.parse_input(inp).map(|output| {
      Located::new(
        inp
          .slice_since(&cursor)
          .expect("parser should guarantee slice"),
        inp.span_since(&cursor),
        output,
      )
    })
  }
}

/// Extension trait for unwrapping `Option` outputs.
pub trait ParseInputUnwrapExt<'inp, L, O, Ctx, Lang: ?Sized, Cmpl = Complete> {
  /// Creates an `Unwrapped` parser that unwraps the `Option` result of this parser.
  #[inline(always)]
  #[track_caller]
  fn unwrap(self) -> Unwrapped<Self, O, Ctx, Lang, Cmpl>
  where
    Self: Sized + ParseInput<'inp, L, Option<O>, Ctx, Lang, Cmpl>,
  {
    Unwrapped::new(self)
  }
}

impl<'inp, F, L, O, Ctx, Lang: ?Sized, Cmpl> ParseInputUnwrapExt<'inp, L, O, Ctx, Lang, Cmpl> for F
where
  F: ParseInput<'inp, L, Option<O>, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
{
}
