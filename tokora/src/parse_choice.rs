use crate::{
  Token, Window,
  cache::Peeked,
  input::InputRef,
  parser::{DispatchOnKind, FusedDispatchOnKind, PeekThenChoice},
  span::Spanned,
  try_parse_input::ParseAttempt,
};

use super::*;

/// A choice of multiple parsers.
pub trait ParseChoice<'inp, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  /// The id of the parser branch.
  type Id;

  /// Parses using branch identified by `id`.
  fn parse_choice(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    id: &Self::Id,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;

  /// Parses using branch identified by `id`.
  #[inline(always)]
  fn try_parse_choice(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    id: Option<&Self::Id>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness,
  {
    match id {
      Some(id) => self.parse_choice(inp, id).map(ParseAttempt::Accept),
      None => Ok(ParseAttempt::Decline),
    }
  }

  /// Creates a `PeekThenChoice` combinator that peeks at most `N` tokens first from the input before parsing.
  ///
  /// If the condition handler `H` returns `Ok(id)`, the inner choice parser is applied with the given id, otherwise,
  /// parsing is stopped and return the error from the handler.
  ///
  /// The handler owns its failure diagnostic — including any `expected one of …` set. To derive
  /// that set automatically from a static table of viable first-token kinds instead, see
  /// [`dispatch_on_kind`](Self::dispatch_on_kind).
  #[inline(always)]
  fn peek_then_choice<H, W: Window>(self, condition: H) -> PeekThenChoice<Self, H, L, Ctx, W, Lang>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    H: FnMut(
      Peeked<'_, 'inp, L, W>,
      &mut Ctx::Emitter,
    ) -> Result<Self::Id, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  {
    PeekThenChoice::of(self, condition)
  }

  /// Creates a `PeekThenChoice` combinator that peeks at most `N` tokens first from the input before parsing.
  ///
  /// If the condition handler `H` returns `Ok(id)`, the inner choice parser is applied with the given id, otherwise,
  /// parsing is stopped and return the error from the handler.
  #[inline(always)]
  fn peek_then_try_choice<H, W: Window>(
    self,
    condition: H,
  ) -> PeekThenChoice<Self, H, L, Ctx, W, Lang>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    H: FnMut(
      Peeked<'_, 'inp, L, W>,
      &mut Ctx::Emitter,
    ) -> Result<Option<Self::Id>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  {
    PeekThenChoice::of(self, condition)
  }

  /// Creates a [`DispatchOnKind`] combinator that dispatches on the kind of the next token
  /// using a **static table** of viable first-token kinds.
  ///
  /// `table[i]` is the viable first-token [`Kind`](Token::Kind) for branch `i`, in branch
  /// order. The combinator peeks a single token, looks its kind up in the table, and runs the
  /// matching branch. On a **committed dispatch failure** — the next token's kind is absent from
  /// the table — the returned [`UnexpectedToken`](crate::error::token::UnexpectedToken) carries
  /// the *whole* table as its expected set (`expected one of …`, an
  /// [`Expected::OneOf`](crate::utils::Expected::OneOf)); at end-of-input it returns an
  /// [`UnexpectedEot`](crate::error::UnexpectedEot). The expected set is exact and never
  /// speculative because the viable set is precisely the table.
  ///
  /// Unlike [`peek_then_choice`](Self::peek_then_choice), whose handler must build any failure
  /// diagnostic by hand, `dispatch_on_kind` derives the expected set from the table automatically.
  /// For many-to-one dispatch (several kinds routing to one branch) use
  /// [`peek_then_choice`](Self::peek_then_choice) instead.
  #[inline(always)]
  fn dispatch_on_kind(
    self,
    table: &'static [<L::Token as Token<'inp>>::Kind],
  ) -> DispatchOnKind<Self, <L::Token as Token<'inp>>::Kind, L, Ctx, Lang>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    DispatchOnKind::of(self, table)
  }
}

/// A choice of parsers whose selected branch receives the **already-lexed** head token.
///
/// `ParseTokenChoice` is the arm surface of the *fused* dispatch shape
/// ([`FusedDispatchOnKind`]). Its peek-shaped sibling [`ParseChoice`] leaves the head
/// token on the input — the dispatcher peeks (staging the token in the cache) and the
/// winning branch consumes it back out. The fused shape instead lexes **once**: the
/// dispatcher consumes the head token as part of classifying it and hands it to the
/// winning branch, which parses only the *rest* of its production from the input. The
/// cache round trip — staging a [`CachedToken`](crate::cache::CachedToken) (including a
/// lexer-state clone) only to unstage it on the very next consume — is skipped entirely.
///
/// Implemented for tuples of up to 32 arms, each an
/// `FnMut(Spanned<Token, Span>, &mut InputRef<…>) -> Result<O, Error>` (a closure or a
/// plain `fn`); branch `i` is tuple position `i`, identified by [`Branch`].
pub trait ParseTokenChoice<'inp, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  /// The id of the parser branch.
  type Id;

  /// Parses using the branch identified by `id`, handing it the already-lexed `head`
  /// token (the token the dispatcher consumed to make its decision).
  fn parse_token_choice(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    id: &Self::Id,
    head: Spanned<L::Token, L::Span>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;

  /// Creates a [`FusedDispatchOnKind`] combinator: the **fused** twin of
  /// [`dispatch_on_kind`](ParseChoice::dispatch_on_kind), driven by the same kind of
  /// **static table** of viable first-token kinds.
  ///
  /// `table[i]` is the viable first-token [`Kind`](Token::Kind) for branch `i`, in branch
  /// order. The combinator lexes a single token **once**, looks its kind up in the table,
  /// and on a hit hands the already-lexed token to the matching branch — no peek round
  /// trip through the cache. When driven as [`ParseInput`], a miss or end of input fails
  /// exactly like [`dispatch_on_kind`](ParseChoice::dispatch_on_kind): the same
  /// [`UnexpectedToken`](crate::error::token::UnexpectedToken) /
  /// [`UnexpectedEot`](crate::error::UnexpectedEot) carrying the whole table as the
  /// expected set, with the missed token put back for whatever runs next. When driven as
  /// [`TryParseInput`], a table miss or end of input instead returns
  /// [`ParseAttempt::Decline`] without consuming valid input; an error from a selected arm
  /// remains an `Err`. See
  /// [`FusedDispatchOnKind`] for the full shape comparison and the equivalence contract.
  #[inline(always)]
  fn fused_dispatch_on_kind(
    self,
    table: &'static [<L::Token as Token<'inp>>::Kind],
  ) -> FusedDispatchOnKind<Self, <L::Token as Token<'inp>>::Kind, L, Ctx, Lang>
  where
    Self: Sized,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    FusedDispatchOnKind::of(self, table)
  }
}

macro_rules! tuple_choice {
  (@output $end:literal; $($param:literal),+ $(,)?) => {
    ::paste::paste! {
      impl<'inp, L, O, Ctx, Lang: ?Sized, $([< P $param >]),+>
        ParseChoice<'inp, L, O, Ctx, Lang>
        for ($([< P $param >],)+)
      where
        L: Lexer<'inp>,
        Ctx: ParseContext<'inp, L, Lang>,
        $([< P $param >]: ParseInput<'inp, L, O, Ctx, Lang>),+
      {
        type Id = Branch<$end>;

        fn parse_choice(
          &mut self,
          inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
          id: &Self::Id,
        ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
          match id.id() {
            $($param => self.$param.parse_input(inp),)+
            _ => unreachable!(concat!("Branch<", stringify!($end), "> guarantees in-bounds")),
          }
        }
      }
    }
  };
  (@mid $end:literal) => {
    seq_macro::seq!(N in 0..=$end {
      tuple_choice!(@output $end; #(N,)*);
    });
  };
  ($end:literal) => {
    seq_macro::seq!(E in 0..=$end {
      tuple_choice!(@mid E);
    });
  };
}

// `ParseChoice`/`TryParseChoice` are implemented for choice tuples from `(P0,)` up to
// `(P0, .., P32)` (the largest being `Branch<32>`). Tuples larger than this are
// unsupported; nest an inner `choice(..)` to exceed the cap.
tuple_choice!(32);

macro_rules! fused_tuple_choice {
  (@output $end:literal; $($param:literal),+ $(,)?) => {
    ::paste::paste! {
      impl<'inp, L, O, Ctx, Lang: ?Sized, $([< F $param >]),+>
        ParseTokenChoice<'inp, L, O, Ctx, Lang>
        for ($([< F $param >],)+)
      where
        L: Lexer<'inp>,
        Ctx: ParseContext<'inp, L, Lang>,
        $([< F $param >]: FnMut(
          Spanned<L::Token, L::Span>,
          &mut InputRef<'inp, '_, L, Ctx, Lang>,
        ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>),+
      {
        type Id = Branch<$end>;

        fn parse_token_choice(
          &mut self,
          inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
          id: &Self::Id,
          head: Spanned<L::Token, L::Span>,
        ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
          match id.id() {
            $($param => (self.$param)(head, inp),)+
            _ => unreachable!(concat!("Branch<", stringify!($end), "> guarantees in-bounds")),
          }
        }
      }
    }
  };
  (@mid $end:literal) => {
    seq_macro::seq!(N in 0..=$end {
      fused_tuple_choice!(@output $end; #(N,)*);
    });
  };
  ($end:literal) => {
    seq_macro::seq!(E in 0..=$end {
      fused_tuple_choice!(@mid E);
    });
  };
}

// `ParseTokenChoice` mirrors the same arities: fused-arm tuples from `(F0,)` up to
// `(F0, .., F32)`, each arm an `FnMut(head, inp) -> Result<O, E>`.
fused_tuple_choice!(32);

/// The array choice's branch ids are **1-based**, and that is what makes an out-of-bounds id
/// unrepresentable rather than merely undocumented.
///
/// `RangedUsize<0, N>`'s bounds are *inclusive*, so it admitted `N` — one past the last index —
/// and dispatching it panicked. `[P; 0]` was worse: `RangedUsize<0, 0>` admits `0`, so **every**
/// id for an empty array panicked. The whole point of a ranged id is that an out-of-range value
/// cannot be built, and an inclusive-upper range over `0..N` cannot deliver that.
///
/// `RangedUsize<1, N>` is the bijection: exactly `N` representable values, `1..=N`, mapping onto
/// the indices `0..N`. Every id that exists is a valid index, so the dispatch's subtraction
/// cannot go out of bounds.
///
/// The obvious alternative — `RangedUsize<0, { N - 1 }>` — is not writable on stable: a
/// generic `N` in a const operation is `error: generic parameters may not be used in const
/// operations`, and `generic_const_exprs` is unstable on every toolchain this crate supports.
///
/// ## `[P; 0]`
///
/// `RangedUsize<1, 0>` is an **empty** range. Declaring the type is fine and an unused `[P; 0]`
/// choice still compiles; *constructing* an id for one is a post-monomorphization const-eval
/// error (`E0080`, from `deranged`'s own range assertion). So an empty array choice moves from
/// "every dispatch panics at runtime" to "no id exists, and building one is a compile error" —
/// unrepresentable-invalid in the strongest form stable Rust offers. The error comes from
/// `deranged`, not from here, which is why it is documented rather than wrapped.
impl<'inp, L, O, Ctx, Lang: ?Sized, P, const N: usize> ParseChoice<'inp, L, O, Ctx, Lang> for [P; N]
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: ParseInput<'inp, L, O, Ctx, Lang>,
{
  type Id = deranged::RangedUsize<1, N>;

  #[inline(always)]
  fn parse_choice(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
    id: &Self::Id,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // `id.get()` is in `1..=N` by construction, so the subtraction lands in `0..N` and the
    // index cannot be out of bounds. This is the bijection doing the work the type promised.
    self[id.get() - 1].parse_input(inp)
  }
}

/// `Id = usize` because a slice's length is a runtime fact — there is no compile-time repair
/// available here, unlike the array impl above.
///
/// What there is instead is a **named** refusal. An out-of-range id used to reach the raw slice
/// index and panic with `index out of bounds`, which says nothing about the contract that was
/// broken. The check below panics with the contract in the message. The bounds are also now
/// documented.
///
/// ## Panics
///
/// If `id` is not a valid index into this slice.
impl<'inp, L, O, Ctx, Lang: ?Sized, P> ParseChoice<'inp, L, O, Ctx, Lang> for [P]
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: ParseInput<'inp, L, O, Ctx, Lang>,
{
  type Id = usize;

  #[inline(always)]
  fn parse_choice(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
    id: &Self::Id,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let len = self.len();
    assert!(*id < len, "choice id {id} out of bounds for {len} branches");
    self[*id].parse_input(inp)
  }
}

/// `Id = usize` because a slice's length is a runtime fact — there is no compile-time repair
/// available here, unlike the array impl above.
///
/// What there is instead is a **named** refusal. An out-of-range id used to reach the raw slice
/// index and panic with `index out of bounds`, which says nothing about the contract that was
/// broken. The check below panics with the contract in the message. The bounds are also now
/// documented.
///
/// ## Panics
///
/// If `id` is not a valid index into this slice.
impl<'inp, L, O, Ctx, Lang: ?Sized, P> ParseChoice<'inp, L, O, Ctx, Lang> for &mut [P]
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: ParseInput<'inp, L, O, Ctx, Lang>,
{
  type Id = usize;

  #[inline(always)]
  fn parse_choice(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
    id: &Self::Id,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let len = self.len();
    assert!(*id < len, "choice id {id} out of bounds for {len} branches");
    self[*id].parse_input(inp)
  }
}

#[cfg(any(feature = "std", feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
const _: () = {
  use std::boxed::Box;

  impl<'inp, L, O, Ctx, T, Lang: ?Sized> ParseChoice<'inp, L, O, Ctx, Lang> for Box<T>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    T: ParseChoice<'inp, L, O, Ctx, Lang>,
  {
    type Id = T::Id;

    #[inline(always)]
    fn parse_choice(
      &mut self,
      inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
      id: &Self::Id,
    ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
      (**self).parse_choice(inp, id)
    }
  }
};

/// Branch identifier for choice parsers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Branch<const N: usize>(usize);

impl<const N: usize> Branch<N> {
  /// Returns the matched branch id.
  #[inline(always)]
  pub const fn id(&self) -> usize {
    self.0
  }

  /// Constructs a branch from a raw index.
  ///
  /// Crate-internal: the caller must guarantee `index <= N` (the in-bounds contract every
  /// `ParseChoice` dispatch relies on). Used by [`DispatchOnKind`](crate::parser::DispatchOnKind)
  /// after a table lookup, where the matched table position is a valid branch index.
  #[inline(always)]
  pub(crate) const fn from_index(index: usize) -> Self {
    debug_assert!(index <= N, "Branch index out of range");
    Branch(index)
  }
}

#[allow(non_upper_case_globals)]
mod sealed {
  use super::Branch;

  macro_rules! bound {
    ($($param: literal),+$(,)?) => {
      paste::paste! {
        $(
          #[doc(hidden)]
          pub trait [< _ $param >] {}
        )*
      }
    };
  }

  seq_macro::seq!(N in 1..=32 {
    bound!(#(N,)*);
  });

  impl<const N: usize> Branch<N> {
    /// The zeroth branch.
    pub const B0: Self = Branch(0);
  }

  macro_rules! const_value {
    ($(
      $(#[$meta:meta])*
      $id:literal
    ),+$(,)?) => {
      paste::paste! {
        $(
          impl<const N: usize> Branch<N>
          where
            Self: [< _ $id >],
          {
            $(#[$meta])*
            pub const [<B $id>]: Self = Branch($id);
          }
        )*
      }
    };
  }

  macro_rules! impl_bound {
    (@inner $end:literal; $($param:literal),+ $(,)?) => {
      ::paste::paste! {
        $(
          impl [< _ $param >] for Branch<$end>
          {}
        )*
      }
    };
    ($end:literal) => {
      paste::paste! {
        seq_macro::seq!(P in 1..=$end {
          impl_bound!(@inner $end; P);
        });
      }
    };
  }

  seq_macro::seq!(E in 1..=32 {
    impl_bound!(E);
  });

  const_value!(
    /// The first branch.
    1,
    /// The second branch.
    2,
    /// The third branch.
    3,
    /// The fourth branch.
    4,
    /// The fifth branch.
    5,
    /// The sixth branch.
    6,
    /// The seventh branch.
    7,
    /// The eighth branch.
    8,
    /// The ninth branch.
    9,
    /// The tenth branch.
    10,
    /// The eleventh branch.
    11,
    /// The twelfth branch.
    12,
    /// The thirteenth branch.
    13,
    /// The fourteenth branch.
    14,
    /// The fifteenth branch.
    15,
    /// The sixteenth branch.
    16,
    /// The seventeenth branch.
    17,
    /// The eighteenth branch.
    18,
    /// The nineteenth branch.
    19,
    /// The twentieth branch.
    20,
    /// The twenty-first branch.
    21,
    /// The twenty-second branch.
    22,
    /// The twenty-third branch.
    23,
    /// The twenty-fourth branch.
    24,
    /// The twenty-fifth branch.
    25,
    /// The twenty-sixth branch.
    26,
    /// The twenty-seventh branch.
    27,
    /// The twenty-eighth branch.
    28,
    /// The twenty-ninth branch.
    29,
    /// The thirtieth branch.
    30,
    /// The thirty-first branch.
    31,
    /// The thirty-second branch.
    32,
  );
}
