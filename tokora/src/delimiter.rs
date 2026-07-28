use crate::{Lexer, Token, error::token::UnexpectedToken, punct::*, span::Spanned, utils::CowStr};

/// The machine identity of a delimiter pair — the discriminator
/// [`FromUnclosed`](crate::emitter::FromUnclosed) matches on.
///
/// # Why this is not [`Delimiter::name`]
///
/// `name` is a **display** string. It is rendered into `unclosed delimiter '{name}'` and it
/// carries no uniqueness contract: two distinct pairs may legitimately want the same one, and
/// `"[]"` is the correct, obvious name for *any* bracket-shaped pair, not just tokora's. Using
/// it to route an [`Unclosed`](crate::error::Unclosed) conversion made the correct display
/// name and a correct dispatch key mutually exclusive. `DelimiterKind` is the dispatch key, so
/// they no longer compete.
///
/// # What this guarantees, and what it does not
///
/// **Guaranteed, by construction:** the four built-in variants are each `#[non_exhaustive]`,
/// so no crate but tokora can *name* one. Written anywhere else, `DelimiterKind::Bracket` is
/// a privacy error (`E0603`) rather than a value — in a [`Delimiter::KIND`] declaration, in
/// the `kind` argument to [`Unclosed::new`](crate::error::Unclosed::new) or
/// [`of`](crate::error::Unclosed::of), in a `const`, anywhere. A custom bracket named `"[]"`
/// routing to the built-in arm is not discouraged — it does not compile.
///
/// That guarantee is about the **spelling**, and it stops there. It removes the *accident*:
/// an author reaching for `Bracket` because `"[]"` was the correct name for their pair. It
/// does not make the value unobtainable: a crate that is not tokora has three ways to obtain
/// one — enumerated under *Not guaranteed* below — and only two of them name anything, the
/// third being a copy of a kind the crate was handed. A `DelimiterKind::Bracket { .. }` arm
/// firing therefore means dispatch was not keyed on a display string. It is not proof of
/// provenance, and it is not proof the caller chose the kind.
///
/// **Matching is untouched**, at a cost of one token: an arm written outside tokora reads
/// `DelimiterKind::Bracket { .. }`, not `DelimiterKind::Bracket`. That is exactly what
/// `#[non_exhaustive]` on a *variant* buys — construction refused, patterns allowed — and it
/// is why the discriminator can stay a matchable enum instead of an opaque identity.
///
/// An outside pair that declares a built-in — **does not compile**:
///
/// ```rust,compile_fail,E0603
/// use tokora::{
///   Lexer,
///   delimiter::{Delimiter, DelimiterKind},
///   punct::{CloseAngle, OpenAngle, Punctuator},
///   utils::CowStr,
/// };
///
/// struct MyPair;
///
/// impl<'inp, L, Lang: ?Sized> Delimiter<'inp, L, Lang> for MyPair
/// where
///   L: Lexer<'inp>,
///   OpenAngle<(), (), Lang>: Punctuator<'inp, L, Lang>,
///   CloseAngle<(), (), Lang>: Punctuator<'inp, L, Lang>,
/// {
///   const KIND: DelimiterKind = DelimiterKind::Bracket;
///
///   type Open = OpenAngle<(), (), Lang>;
///   type Close = CloseAngle<(), (), Lang>;
///
///   fn name() -> CowStr {
///     CowStr::from_static("[]")
///   }
/// }
/// ```
///
/// The same pair keyed by something its own crate owns — compiles:
///
/// ```rust
/// use tokora::{
///   Lexer,
///   delimiter::{Delimiter, DelimiterKind},
///   punct::{CloseAngle, OpenAngle, Punctuator},
///   utils::CowStr,
/// };
///
/// struct MyPair;
///
/// impl<'inp, L, Lang: ?Sized> Delimiter<'inp, L, Lang> for MyPair
/// where
///   L: Lexer<'inp>,
///   OpenAngle<(), (), Lang>: Punctuator<'inp, L, Lang>,
///   CloseAngle<(), (), Lang>: Punctuator<'inp, L, Lang>,
/// {
///   const KIND: DelimiterKind = DelimiterKind::Custom("my_crate::MyPair");
///
///   type Open = OpenAngle<(), (), Lang>;
///   type Close = CloseAngle<(), (), Lang>;
///
///   fn name() -> CowStr {
///     CowStr::from_static("[]")
///   }
/// }
/// ```
///
/// The pair differs in one token, and it is also the tripwire for the day the fence is
/// reopened: drop `#[non_exhaustive]` from a built-in variant and the first example starts
/// compiling, failing its `compile_fail`. Both are compiled as their own crates against
/// tokora, so the privacy they exercise is the one a downstream crate sees — `E0603` cannot
/// be raised from inside tokora, where every built-in is nameable.
///
/// What the pair establishes is exactly one thing: an outside crate cannot **name** a built-in
/// variant. It says nothing about whether an outside crate can **obtain** the value, and the
/// answer to that is yes. The routes are listed next, and each is pinned *green* — asserting
/// that the forgery succeeds, because it does — in `tests/unclosed_kind_dispatch.rs`, itself
/// an integration test and so itself a separate crate. Closing a route breaks a line there
/// instead of quietly making this page wrong.
///
/// **Not guaranteed**, stated exactly:
///
/// - *That two `Custom` payloads differ.* Two pairs defined in one crate can both declare
///   `Custom("mine")`, and nothing here stops them. That collision is a real residue, not a
///   solved problem — but it is a strictly better one than the collision it replaces. It is
///   *visible*: both declarations sit in the crate that owns them, spelled out next to each
///   other, rather than the invisible aliasing of a local pair onto a tokora built-in the
///   author never sees written down. And it *contradicts a stated contract*, where the old
///   collision happened with the author doing exactly what `name`'s documentation asked.
///   Uniqueness moved from invisible to visible-plus-contract; it did not dissolve.
///
/// - *That a built-in is unobtainable — only that it is unnameable.* Unnameable and
///   unobtainable are different properties and only the first is on offer: a `Copy` value a
///   public API hands out can be copied. Three public doors give a built-in kind to a crate
///   that is not tokora, and not one of them names a variant.
///
///   1. **The associated-const projection.** [`KIND`](Delimiter::KIND) is public and its type
///      is unconstrained, so an outside pair can declare `const KIND: DelimiterKind =
///      <tokora::punct::Bracket<…> as Delimiter<…>>::KIND;` over punctuators that are not
///      brackets. Every consumer's `DelimiterKind::Bracket { .. }` arm then takes it.
///   2. **The round trip through [`Unclosed`](crate::error::Unclosed).**
///      [`kind`](crate::error::Unclosed::kind) hands the value out and
///      [`new`](crate::error::Unclosed::new)/[`of`](crate::error::Unclosed::of) take one back,
///      so a kind read off an error tokora built can be put into an error it did not build.
///      This is not a hypothetical shape: tokora's own `examples/json.rs` performs that exact
///      round trip, legitimately, to re-spell a diagnostic it is forwarding.
///   3. **The typed constructors.** [`Unclosed::bracket`](crate::error::Unclosed::bracket),
///      [`bracket_of`](crate::error::Unclosed::bracket_of) and their `paren`/`brace`/`angle`
///      siblings mint a fully formed built-in-kinded error, carrying tokora's own name for the
///      pair, in one call, from any crate, over any span. It is the shortest of the three and
///      it needs no [`Delimiter`] impl at all.
///
///   Doors 1 and 3 have to name tokora's own pair in the author's own source. Door 2 does not:
///   a generic adapter that re-spells an error it is forwarding — `Unclosed::of(outer,
///   err.kind(), err.name())` — copies whatever kind it was handed without ever naming a
///   variant, a pair, or a constructor. So a built-in arm firing does not mean the caller
///   chose it. What the fence removes is dispatch keyed on a display string, and nothing more
///   than that was ever on offer: this is a discriminator, not a provenance signal, and it must
///   not be used to decide trust.
///
/// # Why those doors are left open
///
/// Door 1 is closable on its own, and it was measured rather than assumed. Moving the identity
/// into a `#[doc(hidden)]` provided method whose parameter type is unnameable outside tokora —
/// an outside impl cannot spell the signature, so it cannot override the default, so it always
/// reports `Custom` — compiles, keeps the `&D`/`&mut D` forwarding impls, and refuses both the
/// override and the projection. The shape that looks more obvious, a sealed supertrait holding
/// `KIND` with a blanket impl over a public key trait, does **not** compile: it collides with
/// the `&D` forwarding impl (`E0119`, *downstream crates may implement trait `…` for type
/// `&_`*), because nothing stops a downstream crate implementing the key trait for a reference
/// to its own type.
///
/// Closing door 1 alone buys no sentence on this page, because doors 2 and 3 stay open and
/// door 3 is one public call. Closing all three means making the eight typed constructors
/// crate-private and reshaping `new`/`of` so they cannot accept a `DelimiterKind` at all —
/// which means deriving it from a `D: Delimiter` type parameter, which makes `Unclosed<char>`
/// unconstructible outside tokora and requires a `Delimiter` impl to exist before any unclosed
/// diagnostic can be built by hand. That is ten public items removed or reshaped, and the
/// collapse of the delimiter parameter's documented range (`char`, `&'static str`, a custom
/// enum), to fence a discriminant whose worst case is a misrouted error message.
///
/// The trade is refused, and a partial closure is refused with it: moving the API without
/// making any stronger sentence true here, while reading like enforcement, is the exact
/// failure this type was introduced to fix, repeated one level up.
///
/// A payload of [`core::any::TypeId`] would close the residue, since types are globally
/// unique. It is deliberately not used: `TypeId` cannot appear in a `match` pattern, so every
/// consumer would write an `if`/`else if` chain of `TypeId::of::<T>()` comparisons on their
/// hottest error-handling surface, and it would impose `D: 'static` on custom markers, which
/// excludes legitimate borrowing pairs.
///
/// # Choosing a payload
///
/// Use something the defining crate already owns and would not duplicate — a crate-qualified
/// path is the safe habit:
///
/// ```rust
/// use tokora::delimiter::DelimiterKind;
///
/// const DOC_COMMENT: DelimiterKind = DelimiterKind::Custom("my_crate::DocComment");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DelimiterKind {
  /// The built-in `()` pair.
  ///
  /// `#[non_exhaustive]` so that only tokora can write it; match it from outside as
  /// `DelimiterKind::Paren { .. }`.
  #[non_exhaustive]
  Paren,
  /// The built-in `<>` pair.
  ///
  /// `#[non_exhaustive]` so that only tokora can write it; match it from outside as
  /// `DelimiterKind::Angle { .. }`.
  #[non_exhaustive]
  Angle,
  /// The built-in `[]` pair.
  ///
  /// `#[non_exhaustive]` so that only tokora can write it; match it from outside as
  /// `DelimiterKind::Bracket { .. }`.
  #[non_exhaustive]
  Bracket,
  /// The built-in `{}` pair.
  ///
  /// `#[non_exhaustive]` so that only tokora can write it; match it from outside as
  /// `DelimiterKind::Brace { .. }`.
  #[non_exhaustive]
  Brace,
  /// Any pair tokora does not define — and the only variant a `Delimiter` impl outside
  /// tokora can *name*.
  ///
  /// The payload is the defining crate's own key. Keep it unique **within the crate that
  /// defines the pair**; tokora guarantees only that a `Custom` never equals a built-in. That
  /// is a guarantee about this variant, not about the others: an outside impl that declines to
  /// use it can still obtain a built-in value without naming one. See the type-level docs for
  /// those routes and for exactly how far the guarantee reaches.
  Custom(&'static str),
}

/// A trait for any delimiter consisting of an opening and a closing punctuator.
///
/// # The language brand is a fence
///
/// Every built-in pair marker carries its own brand — `Bracket<S, C, Lang>` — and the blanket
/// impls generated for the four of them name that brand and the context language as the
/// **same** parameter. A pair branded for one grammar is therefore not a delimiter of another:
/// a production copy-pasted out of a sibling dialect fails to compile instead of quietly
/// type-checking and then driving to completion on the wrong dialect's marker.
///
/// This costs the fluent families nothing. `delimited_by_brackets`,
/// `try_delimited_by_brackets` and their siblings — on [`ParseInput`](crate::ParseInput) and on
/// every many-builder — stay reachable at a branded language because the macros that generate
/// them instantiate the pair at the *caller's* `Lang`, not at a bare `Bracket<(), (), ()>` that
/// a widened impl would have to accept. What a branded grammar does spell out is a bound it
/// forwards through a **generic** lexer: `Bracket<(), (), Lang>: TypedDelimiter<'inp, L, Lang>`
/// rather than `Bracket: …`. At a concrete lexer the impl resolves and nothing is written.
///
/// The next three examples are the record. Each of the two that do not compile differs from
/// the one that does in exactly one token.
///
/// A pair branded for another language is not a `Delimiter` of this one — **does not compile**:
///
/// ```rust,compile_fail,E0277
/// use tokora::{
///   Lexer, Token,
///   delimiter::Delimiter,
///   punct::{Bracket, CloseBracket, OpenBracket},
/// };
///
/// struct LangA;
/// struct LangB;
///
/// fn assert_delim<'inp, L, D, Lang>()
/// where
///   L: Lexer<'inp>,
///   D: Delimiter<'inp, L, Lang>,
/// {
/// }
///
/// fn probe<'inp, L>()
/// where
///   L: Lexer<'inp>,
///   <L::Token as Token<'inp>>::Kind: From<OpenBracket<(), (), ()>> + From<CloseBracket<(), (), ()>>,
/// {
///   assert_delim::<L, Bracket<(), (), LangA>, LangA>();
///   assert_delim::<L, Bracket<(), (), LangA>, LangB>();
/// }
/// ```
///
/// Nor a [`TypedDelimiter`] of it — **does not compile**:
///
/// ```rust,compile_fail,E0277
/// use tokora::{
///   Lexer, Token,
///   delimiter::TypedDelimiter,
///   punct::{Bracket, CloseBracket, OpenBracket},
/// };
///
/// struct LangA;
/// struct LangB;
///
/// fn assert_typed<'inp, L, D, Lang>()
/// where
///   L: Lexer<'inp>,
///   D: TypedDelimiter<'inp, L, Lang>,
/// {
/// }
///
/// fn probe<'inp, L>()
/// where
///   L: Lexer<'inp>,
///   <L::Token as Token<'inp>>::Kind: From<OpenBracket<(), (), ()>> + From<CloseBracket<(), (), ()>>,
/// {
///   assert_typed::<L, Bracket<(), (), LangA>, LangA>();
///   assert_typed::<L, Bracket<(), (), LangA>, LangB>();
/// }
/// ```
///
/// The same two probes with each pair branded for the grammar asking for it — compiles:
///
/// ```rust
/// use tokora::{
///   Lexer, Token,
///   delimiter::{Delimiter, TypedDelimiter},
///   punct::{Bracket, CloseBracket, OpenBracket},
/// };
///
/// struct LangA;
/// struct LangB;
///
/// fn assert_delim<'inp, L, D, Lang>()
/// where
///   L: Lexer<'inp>,
///   D: Delimiter<'inp, L, Lang>,
/// {
/// }
///
/// fn assert_typed<'inp, L, D, Lang>()
/// where
///   L: Lexer<'inp>,
///   D: TypedDelimiter<'inp, L, Lang>,
/// {
/// }
///
/// fn probe<'inp, L>()
/// where
///   L: Lexer<'inp>,
///   <L::Token as Token<'inp>>::Kind: From<OpenBracket<(), (), ()>> + From<CloseBracket<(), (), ()>>,
/// {
///   assert_delim::<L, Bracket<(), (), LangA>, LangA>();
///   assert_delim::<L, Bracket<(), (), LangB>, LangB>();
///   assert_typed::<L, Bracket<(), (), LangA>, LangA>();
///   assert_typed::<L, Bracket<(), (), LangB>, LangB>();
/// }
/// ```
///
/// The trio is also the tripwire for the day the fence is reopened: widen either impl back to
/// an independent marker-language parameter and the matching example starts compiling, failing
/// its `compile_fail`. Widening `TypedDelimiter` alone cannot reopen anything — its
/// [`Delimiter`] supertrait still fences it — so the first example is the one with teeth on
/// the reachable defect and the second records that the subtrait moved with it.
pub trait Delimiter<'inp, L, Lang: ?Sized = ()> {
  /// The opening punctuator.
  type Open: Punctuator<'inp, L, Lang>;
  /// The closing punctuator.
  type Close: Punctuator<'inp, L, Lang>;

  /// This pair's machine identity — what an unclosed-delimiter conversion discriminates on.
  ///
  /// Distinct from [`name`](Delimiter::name), which is for humans and has no uniqueness
  /// contract. A pair defined outside tokora declares
  /// [`DelimiterKind::Custom`] with a key its own crate owns:
  ///
  /// ```rust,ignore
  /// const KIND: DelimiterKind = DelimiterKind::Custom("my_crate::DocComment");
  /// ```
  ///
  /// It is the only variant such a pair can *name*: the four built-ins are
  /// `#[non_exhaustive]`, so spelling one from another crate is a privacy error rather than a
  /// value. Naming is not the only way to obtain one, though — this const is public and its
  /// type is unconstrained, so a pair can also set its identity from
  /// `<tokora::punct::Bracket<…> as Delimiter<…>>::KIND` and be routed to a consumer's
  /// built-in arm. [`DelimiterKind`] carries the compiling and non-compiling record of the
  /// fence, that route and the two others like it, and why they are left open.
  ///
  /// There is deliberately no default. A defaulted `KIND` would let a pair ship with an
  /// identity its author never chose, which is the failure this constant exists to remove.
  const KIND: DelimiterKind;

  /// The name of the delimiter.
  ///
  /// This is a **display** string — it is what `unclosed delimiter '…'` renders — and it is
  /// explicitly *not* an identity. Two pairs may share a name. To discriminate between pairs,
  /// match on [`KIND`](Delimiter::KIND) or [`Unclosed::kind`](crate::error::Unclosed::kind)
  /// instead — read [`KIND`](Delimiter::KIND) for what that does and does not guarantee.
  fn name() -> CowStr;

  /// Checks if the given token kind is the opening delimiter.
  #[inline(always)]
  fn is_open(knd: &<L::Token as Token<'inp>>::Kind) -> bool
  where
    L: Lexer<'inp>,
  {
    <Self::Open as Punctuator<'inp, L, Lang>>::eval(knd)
  }

  /// Checks if the given token kind is the closing delimiter.
  #[inline(always)]
  fn is_close(knd: &<L::Token as Token<'inp>>::Kind) -> bool
  where
    L: Lexer<'inp>,
  {
    <Self::Close as Punctuator<'inp, L, Lang>>::eval(knd)
  }

  /// Creates an `UnexpectedToken` error for an unexpected opening token.
  #[inline(always)]
  fn unexpected_open_token(
    tok: Spanned<L::Token, L::Span>,
  ) -> UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>
  where
    L: Lexer<'inp>,
  {
    <Self::Open as Punctuator<'inp, L, Lang>>::unexpected_token(tok)
  }

  /// Creates an `UnexpectedToken` error for an unexpected closing token.
  #[inline(always)]
  fn unexpected_close_token(
    tok: Spanned<L::Token, L::Span>,
  ) -> UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>
  where
    L: Lexer<'inp>,
  {
    <Self::Close as Punctuator<'inp, L, Lang>>::unexpected_token(tok)
  }
}

/// A [`Delimiter`] whose punctuators can be materialized as typed, span-carrying values —
/// the capability the [`delimited`](crate::parser::delimited) shape parser uses to build
/// its [`Delimited`](crate::utils::Delimited) result.
///
/// The base [`Delimiter`] trait is a classifier and error helper; this additive subtrait
/// adds the *consumption* side, turning a committed opener's or closer's span into the
/// span-carrying punctuator value the shape parsers store. It is implemented for the
/// built-in pairs [`Paren`], [`Brace`], [`Bracket`], and [`Angle`].
///
/// # Custom delimiter pairs
///
/// Any user pair works the same way: implement [`Punctuator`] for the two punctuator types
/// (or define them with [`punctuator!`](crate::punctuator)), then implement [`Delimiter`]
/// and `TypedDelimiter` for the pair. It then drops straight into
/// [`delimited::<MyPair, …>`](crate::parser::delimited) — see that function's example.
///
/// # The language brand is a fence
///
/// The four built-in pairs' `TypedDelimiter` impls name the marker's brand and the context
/// language as the same parameter, exactly as their [`Delimiter`] impls do — a pair branded
/// for one grammar is not a typed delimiter of another. [`Delimiter`] carries the compiling
/// and non-compiling record for both.
pub trait TypedDelimiter<'inp, L, Lang: ?Sized = ()>: Delimiter<'inp, L, Lang>
where
  L: Lexer<'inp>,
{
  /// The span-carrying opening punctuator value this delimiter materializes.
  type OpenValue;
  /// The span-carrying closing punctuator value this delimiter materializes.
  type CloseValue;

  /// Materializes the opening punctuator value from its committed token's span.
  fn open_value(span: L::Span) -> Self::OpenValue;

  /// Materializes the closing punctuator value from its committed token's span.
  fn close_value(span: L::Span) -> Self::CloseValue;
}

macro_rules! impl_builtin_delimiter {
  ($($name:ident { description: $description:literal, open: $open:ident, close: $close:ident $(,)? }),+$(,)?) => {
    $(
      // The pair marker's own brand and the context language are the *same* parameter: a pair
      // branded for one grammar is not a delimiter of another. The fluent `delimited_by_*`
      // family reaches a branded grammar by instantiating the pair at the caller's `Lang`, not
      // by widening this impl. The fence is pinned by the `compile_fail` doctest on the
      // `Delimiter` trait.
      impl<'inp, S, C, L, Lang: ?Sized> Delimiter<'inp, L, Lang>
        for $name<S, C, Lang>
      where
        L: Lexer<'inp>,
        $open<S, C, Lang>: Punctuator<'inp, L, Lang>,
        $close<S, C, Lang>: Punctuator<'inp, L, Lang>,
      {
        const KIND: DelimiterKind = DelimiterKind::$name;

        type Open = $open<S, C, Lang>;

        type Close = $close<S, C, Lang>;

        #[inline(always)]
        fn name() -> CowStr {
          CowStr::from_static($description)
        }
      }

      impl<'inp, S, C, L, Lang: ?Sized> TypedDelimiter<'inp, L, Lang>
        for $name<S, C, Lang>
      where
        L: Lexer<'inp>,
        $open<S, C, Lang>: Punctuator<'inp, L, Lang>,
        $close<S, C, Lang>: Punctuator<'inp, L, Lang>,
      {
        type OpenValue = $open<L::Span, (), Lang>;

        type CloseValue = $close<L::Span, (), Lang>;

        #[inline(always)]
        fn open_value(span: L::Span) -> Self::OpenValue {
          $open::new(span).change_language()
        }

        #[inline(always)]
        fn close_value(span: L::Span) -> Self::CloseValue {
          $close::new(span).change_language()
        }
      }
    )*
  };
}

impl_builtin_delimiter! {
  Paren { description: "()", open: OpenParen, close: CloseParen },
  Angle { description: "<>", open: OpenAngle, close: CloseAngle },
  Bracket { description: "[]", open: OpenBracket, close: CloseBracket },
  Brace { description: "{}", open: OpenBrace, close: CloseBrace },
}

macro_rules! impl_deref {
  (@impl<$ty:ty>) => {
    const KIND: DelimiterKind = <$ty>::KIND;

    type Open = <$ty>::Open;
    type Close = <$ty>::Close;

    #[inline(always)]
    fn name() -> CowStr {
      <$ty>::name()
    }

    #[inline(always)]
    fn is_open(knd: &<<L>::Token as Token<'inp>>::Kind) -> bool
    where
      L: Lexer<'inp>,
    {
      <$ty>::is_open(knd)
    }

    #[inline(always)]
    fn is_close(knd: &<<L>::Token as Token<'inp>>::Kind) -> bool
    where
      L: Lexer<'inp>,
    {
      <$ty>::is_close(knd)
    }

    #[inline(always)]
    fn unexpected_open_token(
      tok: Spanned<L::Token, L::Span>,
    ) -> UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>
    where
      L: Lexer<'inp>,
    {
      <$ty>::unexpected_open_token(tok)
    }

    #[inline(always)]
    fn unexpected_close_token(
      tok: Spanned<L::Token, L::Span>,
    ) -> UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>
    where
      L: Lexer<'inp>,
    {
      <$ty>::unexpected_close_token(tok)
    }
  };
}

impl<'inp, L, Lang: ?Sized, D: ?Sized> Delimiter<'inp, L, Lang> for &D
where
  L: Lexer<'inp>,
  D: Delimiter<'inp, L, Lang>,
{
  impl_deref!(@impl<D>);
}

impl<'inp, L, Lang: ?Sized, D: ?Sized> Delimiter<'inp, L, Lang> for &mut D
where
  L: Lexer<'inp>,
  D: Delimiter<'inp, L, Lang>,
{
  impl_deref!(@impl<D>);
}
