/// The `delimited*` surface every many-builder carries.
///
/// A built-in pair marker is branded — `Paren<S, C, Lang>` — and its `Delimiter` impl names
/// that brand and the driving context's language as the *same* parameter, so the fluent
/// `delimited_by_*` family has to instantiate the pair at the caller's language rather than at
/// a bare `Paren<(), (), ()>` that only an unbranded grammar can drive.
///
/// Which language that is depends on the builder:
///
/// - `$lang:ident` — the builder's own type carries the parameter (`Repeated<…, Lang, …>` and
///   the three other repetition drivers), so the pair is pinned to it right here.
/// - no argument — the option builders are `AtLeast<P>` and friends, generic only over the
///   parser they wrap, so there is no language to name at the impl block. The pair's brand
///   becomes a method parameter instead and the `Delim: Delimiter<'inp, L, Lang>` obligation on
///   the driving impl unifies it with the context language. Same fence, resolved one frame
///   later; a builder stored without ever being driven has to name the brand itself.
macro_rules! define_many_delimited_methods {
  ($lang:ident) => {
    define_many_delimited_methods!(@emit [] [$lang]);
  };
  () => {
    define_many_delimited_methods!(@emit [<Lang: ?Sized>] [Lang]);
  };
  (@emit [$($generics:tt)*] [$lang:ident]) => {
    /// Delimits the parser with the given delimiter.
    #[inline(always)]
    pub const fn delimited<Delim>(self) -> $crate::parser::DelimitedBy<Self, Delim> {
      $crate::parser::DelimitedBy::<Self, Delim>::new(self)
    }

    /// Delimits the parser with parentheses.
    #[inline(always)]
    pub const fn delimited_by_parens $($generics)* (
      self,
    ) -> $crate::parser::DelimitedBy<Self, $crate::punct::Paren<(), (), $lang>> {
      self.delimited::<$crate::punct::Paren<(), (), $lang>>()
    }

    /// Delimits the parser with braces.
    #[inline(always)]
    pub const fn delimited_by_braces $($generics)* (
      self,
    ) -> $crate::parser::DelimitedBy<Self, $crate::punct::Brace<(), (), $lang>> {
      self.delimited::<$crate::punct::Brace<(), (), $lang>>()
    }

    /// Delimits the parser with brackets.
    #[inline(always)]
    pub const fn delimited_by_brackets $($generics)* (
      self,
    ) -> $crate::parser::DelimitedBy<Self, $crate::punct::Bracket<(), (), $lang>> {
      self.delimited::<$crate::punct::Bracket<(), (), $lang>>()
    }

    /// Delimits the parser with angle brackets.
    #[inline(always)]
    pub const fn delimited_by_angles $($generics)* (
      self,
    ) -> $crate::parser::DelimitedBy<Self, $crate::punct::Angle<(), (), $lang>> {
      self.delimited::<$crate::punct::Angle<(), (), $lang>>()
    }
  };
}

/// Generates 4 `ParseInput` impl blocks for `sep/parse/` leaf files.
///
/// Due to `macro_rules!` hygiene, `self` cannot be passed through call-site token trees.
/// Instead, blocks 1+2 use depth-based variant dispatch (`@map_self`/`@map_primary`),
/// and blocks 3+4 use dispatch by `(cardinality, [policy_types])` (`@block3`/`@block4`).
macro_rules! impl_separated_parse {
  // ── @inline helper ───────────────────────────────────────────────────
  (@inline true $($item:tt)*) => { #[inline(always)] $($item)* };
  (@inline false $($item:tt)*) => { $($item)* };

  // ── @map_self: map_parser chain for block 1 ─────────────────────────
  (@map_self 0 $self:ident) => { $self.as_mut().map_parser(|p| p.as_mut()) };
  (@map_self 1 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_self 2 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_self 3 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };

  // ── @map_primary: map_parser chain for block 2 ──────────────────────
  (@map_primary 0 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.as_mut()) };
  (@map_primary 1 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_primary 2 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_primary 3 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };

  // ── @block3: block 3 body dispatch ──────────────────────────────────
  // depth=0, no policy
  (@block3 unbounded [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let f = parser.fn_mut();
    Wrapper(Collect::new(Separated::new::<Sep>(&mut **f), &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let minimum = parser.minimum();
    let f = parser.parser_mut().fn_mut();
    let parser = AtLeast::new(Separated::new::<Sep>(&mut **f), minimum.get());
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let maximum = parser.maximum();
    let f = parser.parser_mut().fn_mut();
    let parser = AtMost::new(Separated::new::<Sep>(&mut **f), maximum.get());
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let maximum = parser.maximum();
    let minimum = parser.minimum();
    let f = parser.parser_mut().fn_mut();
    let parser = Bounded::new(Separated::new::<Sep>(&mut **f), maximum.get(), minimum.get());
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // depth=1, single policy
  (@block3 unbounded [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let f = parser.parser_mut().fn_mut();
    let parser = $p1::new(Separated::new::<Sep>(&mut *f));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut();
    let minimum = inner.minimum();
    let f = inner.parser_mut().fn_mut();
    let parser = $p1::new(AtLeast::new(Separated::new::<Sep>(&mut **f), minimum.get()));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut();
    let maximum = inner.maximum();
    let f = inner.parser_mut().fn_mut();
    let parser = $p1::new(AtMost::new(Separated::new::<Sep>(&mut **f), maximum.get()));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut();
    let maximum = inner.maximum();
    let minimum = inner.minimum();
    let f = inner.parser_mut().fn_mut();
    let parser = $p1::new(Bounded::new(Separated::new::<Sep>(&mut **f), maximum.get(), minimum.get()));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // depth=2, double policy
  (@block3 unbounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let f = parser.parser_mut().parser_mut().fn_mut();
    let parser = $p1::new($p2::new(Separated::new::<Sep>(&mut *f)));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut().parser_mut();
    let minimum = inner.minimum();
    let f = inner.parser_mut().fn_mut();
    let parser = $p1::new($p2::new(AtLeast::new(Separated::new::<Sep>(&mut **f), minimum.get())));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut().parser_mut();
    let maximum = inner.maximum();
    let f = inner.parser_mut().fn_mut();
    let parser = $p1::new($p2::new(AtMost::new(Separated::new::<Sep>(&mut **f), maximum.get())));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut().parser_mut();
    let maximum = inner.maximum();
    let minimum = inner.minimum();
    let f = inner.parser_mut().fn_mut();
    let parser = $p1::new($p2::new(Bounded::new(Separated::new::<Sep>(&mut **f), maximum.get(), minimum.get())));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // ── @block4: block 4 body dispatch ──────────────────────────────────
  // depth=0, no policy
  (@block4 unbounded [] $self:ident $inp:ident) => {{
    const HANDLER: &Unbounded = &Unbounded;
    let (parser, container) = $self.0.parts_mut();
    parser.parse($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let minimum = parser.minimum();
    parser.parser_mut().parse($inp, container, &minimum, &minimum, &minimum)
  }};
  (@block4 at_most [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = parser.maximum();
    parser.parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 bounded [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = parser.to_with();
    parser.parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};

  // depth=1, single policy
  (@block4 unbounded [$p1:ident] $self:ident $inp:ident) => {{
    const HANDLER: &$p1<Unbounded> = &$p1::new(Unbounded);
    let (parser, container) = $self.0.parts_mut();
    parser.parser_mut().parse($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.minimum());
    parser.parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 at_most [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.maximum());
    parser.parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 bounded [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.to_with());
    parser.parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};

  // depth=2, double policy
  (@block4 unbounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    const HANDLER: &$p1<$p2<Unbounded>> = &$p1::new($p2::new(Unbounded));
    let (parser, container) = $self.0.parts_mut();
    parser.parser_mut().parser_mut().parse($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.minimum()));
    parser.parser_mut().parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 at_most [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.maximum()));
    parser.parser_mut().parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 bounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.to_with()));
    parser.parser_mut().parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};

  // ── Main entry point ────────────────────────────────────────────────
  (
    owned_type = [$($owned:tt)*],
    ref_type = [$($reft:tt)*],
    wrapper_type = [$($wt:tt)*],
    map_depth = $depth:tt,
    cardinality = $card:ident,
    policy = [$($policy:ident),*],
    emitters = {$($emitters:tt)*},
    block3_inline = $b3i:ident,
    block4_inline = $b4i:ident $(,)?
  ) => {
    // Block 1: owned -> Container
    impl<'inp, L, F, Sep, O, Container, Ctx, Lang: ?Sized, Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>>
      ParseInput<'inp, L, Container, Ctx, Lang, Cmpl>
      for Collect<$($owned)*, Container, Ctx, Lang, Cmpl>
    where
      L: Lexer<'inp>,
      F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        $($emitters)*,
      // The separator-slot decision gate surfaces a terminal scanner stop as this end-of-input
      // error (`try_expect_or_stop`); the plain separated families carry the bound the delimited
      // ones already required.
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: Default + ContainerT<O> + SeparatorHandler<'inp, L>,
    {
      #[inline(always)]
      fn parse_input(
        &mut self,
        inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
      ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
        Wrapper(impl_separated_parse!(@map_self $depth self))
          .parse_input(inp)
          .map(|_| mem::take(&mut self.container))
      }
    }

    // Block 2: owned -> Spanned<Container>
    impl<'inp, L, F, Sep, O, Container, Ctx, Lang: ?Sized, Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>>
      ParseInput<'inp, L, Spanned<Container, L::Span>, Ctx, Lang, Cmpl>
      for With<Collect<$($owned)*, Container, Ctx, Lang, Cmpl>, PhantomSpan>
    where
      L: Lexer<'inp>,
      F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        $($emitters)*,
      // The separator-slot decision gate surfaces a terminal scanner stop as this end-of-input
      // error (`try_expect_or_stop`); the plain separated families carry the bound the delimited
      // ones already required.
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: Default + ContainerT<O> + SeparatorHandler<'inp, L>,
    {
      #[inline(always)]
      fn parse_input(
        &mut self,
        inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
      ) -> Result<Spanned<Container, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
        Wrapper(impl_separated_parse!(@map_primary $depth self))
          .parse_input(inp)
          .map(|span| Spanned::new(span, mem::take(&mut self.primary.container)))
      }
    }

    // Block 3: &mut ref -> L::Span
    impl<'inp, 'c, L, F, Sep, O, Container, Ctx, Lang: ?Sized, Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>>
      ParseInput<'inp, L, L::Span, Ctx, Lang, Cmpl>
      for Collect<&'c mut $($reft)*, &'c mut Container, Ctx, Lang, Cmpl>
    where
      L: Lexer<'inp>,
      F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        $($emitters)*,
      // The separator-slot decision gate surfaces a terminal scanner stop as this end-of-input
      // error (`try_expect_or_stop`); the plain separated families carry the bound the delimited
      // ones already required.
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: ContainerT<O> + SeparatorHandler<'inp, L>,
    {
      impl_separated_parse!(@inline $b3i
        fn parse_input(
          &mut self,
          input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
        ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
        where
          L: Lexer<'inp>,
          Ctx: ParseContext<'inp, L, Lang>,
        {
          impl_separated_parse!(@block3 $card [$($policy),*] self input)
        }
      );
    }

    struct Wrapper<T>(T);

    // Block 4: Wrapper -> L::Span
    impl<'inp, 'c, L, F, Sep, O, Container, Ctx, Lang: ?Sized, Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>>
      ParseInput<'inp, L, L::Span, Ctx, Lang, Cmpl>
      for Wrapper<Collect<$($wt)*, &'c mut Container, Ctx, Lang, Cmpl>>
    where
      L: Lexer<'inp>,
      F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        $($emitters)*,
      // The separator-slot decision gate surfaces a terminal scanner stop as this end-of-input
      // error (`try_expect_or_stop`); the plain separated families carry the bound the delimited
      // ones already required.
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: ContainerT<O> + SeparatorHandler<'inp, L>,
    {
      impl_separated_parse!(@inline $b4i
        fn parse_input(
          &mut self,
          inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
        ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
          impl_separated_parse!(@block4 $card [$($policy),*] self inp)
        }
      );
    }
  };
}

/// Generates 4 `ParseInput` impl blocks for `sep/delim/` leaf files.
///
/// Same structure as `impl_separated_parse!` but with delim-specific adaptations:
/// - Extra generic `Delim` with `Delimiter<'inp, L, Lang>` bound
/// - Extra error bound `Error: From<UnexpectedEot<L::Offset, Lang>>`
/// - Extra container trait `DelimiterHandler<'inp, L>`
/// - Block 3 uses `delim.parser` field access pattern
/// - Block 4 reconstructs `DelimitedBy::<_, Delim>::new(...)` and calls `.parse_separated()`
macro_rules! impl_separated_delim {
  // ── @inline helper ───────────────────────────────────────────────────
  (@inline true $($item:tt)*) => { #[inline(always)] $($item)* };
  (@inline false $($item:tt)*) => { $($item)* };

  // ── @map_self: map_parser chain for block 1 ─────────────────────────
  (@map_self 1 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_self 2 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_self 3 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };
  (@map_self 4 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))))) };

  // ── @map_primary: map_parser chain for block 2 ──────────────────────
  (@map_primary 1 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_primary 2 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_primary 3 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };
  (@map_primary 4 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))))) };

  // ── @block3: block 3 body dispatch ──────────────────────────────────
  // depth=0, no policy
  (@block3 unbounded [] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let f = delim.parser.fn_mut();
    let parser = DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let minimum = delim.parser.minimum();
    let f = delim.parser.parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new(AtLeast::new(Separated::new::<Sep>(&mut **f), minimum.get()));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let maximum = delim.parser.maximum();
    let f = delim.parser.parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new(AtMost::new(Separated::new::<Sep>(&mut **f), maximum.get()));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let maximum = delim.parser.maximum();
    let minimum = delim.parser.minimum();
    let f = delim.parser.parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new(Bounded::new(Separated::new::<Sep>(&mut **f), maximum.get(), minimum.get()));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // depth=1, single policy
  (@block3 unbounded [$p1:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let f = delim.parser.parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new(Separated::new::<Sep>(&mut **f)));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [$p1:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut();
    let minimum = inner.minimum();
    let f = inner.parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new(AtLeast::new(Separated::new::<Sep>(&mut **f), minimum.get())));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [$p1:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut();
    let maximum = inner.maximum();
    let f = inner.parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new(AtMost::new(Separated::new::<Sep>(&mut **f), maximum.get())));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [$p1:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut();
    let maximum = inner.maximum();
    let minimum = inner.minimum();
    let f = inner.parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new(Bounded::new(Separated::new::<Sep>(&mut **f), maximum.get(), minimum.get())));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // depth=2, double policy
  (@block3 unbounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let f = delim.parser.parser_mut().parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new($p2::new(Separated::new::<Sep>(&mut **f))));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut().parser_mut();
    let minimum = inner.minimum();
    let f = inner.parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new($p2::new(AtLeast::new(Separated::new::<Sep>(&mut **f), minimum.get()))));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut().parser_mut();
    let maximum = inner.maximum();
    let f = inner.parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new($p2::new(AtMost::new(Separated::new::<Sep>(&mut **f), maximum.get()))));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut().parser_mut();
    let maximum = inner.maximum();
    let minimum = inner.minimum();
    let f = inner.parser_mut().fn_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new($p2::new(Bounded::new(Separated::new::<Sep>(&mut **f), maximum.get(), minimum.get()))));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // ── @block4: block 4 body dispatch ──────────────────────────────────
  // depth=0, no policy
  (@block4 unbounded [] $self:ident $inp:ident) => {{
    const HANDLER: &Unbounded = &Unbounded;
    let (parser, container) = $self.0.parts_mut();
    let f = parser.parser.fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let minimum = parser.parser.minimum();
    let f = parser.parser.parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, &minimum, &minimum, &minimum)
  }};
  (@block4 at_most [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let maximum = parser.parser.maximum();
    let f = parser.parser.parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, &maximum, &maximum, &maximum)
  }};
  (@block4 bounded [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = parser.parser.to_with();
    let f = parser.parser.parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};

  // depth=1, single policy
  (@block4 unbounded [$p1:ident] $self:ident $inp:ident) => {{
    const HANDLER: &$p1<Unbounded> = &$p1::new(Unbounded);
    let (parser, container) = $self.0.parts_mut();
    let f = parser.parser.parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.parser.minimum());
    let f = parser.parser.parser_mut().parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 at_most [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.parser.maximum());
    let f = parser.parser.parser_mut().parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 bounded [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.parser.to_with());
    let f = parser.parser.parser_mut().parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};

  // depth=2, double policy
  (@block4 unbounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    const HANDLER: &$p1<$p2<Unbounded>> = &$p1::new($p2::new(Unbounded));
    let (parser, container) = $self.0.parts_mut();
    let f = parser.parser.parser_mut().parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.parser.minimum()));
    let f = parser.parser.parser_mut().parser_mut().parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 at_most [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.parser.maximum()));
    let f = parser.parser.parser_mut().parser_mut().parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 bounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.parser.to_with()));
    let f = parser.parser.parser_mut().parser_mut().parser_mut().fn_mut();
    DelimitedBy::<_, Delim>::new(Separated::new::<Sep>(&mut **f))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};

  // ── Main entry point ────────────────────────────────────────────────
  (
    owned_type = [$($owned:tt)*],
    ref_type = [$($reft:tt)*],
    wrapper_type = [$($wt:tt)*],
    map_depth = $depth:tt,
    cardinality = $card:ident,
    policy = [$($policy:ident),*],
    emitters = {$($emitters:tt)*},
    block3_inline = $b3i:ident,
    block4_inline = $b4i:ident $(,)?
  ) => {
    // Block 1: owned -> Container
    impl<'inp, L, F, Sep, O, Delim, Container, Ctx, Lang: ?Sized, Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>>
      ParseInput<'inp, L, Container, Ctx, Lang, Cmpl>
      for Collect<$($owned)*, Container, Ctx, Lang, Cmpl>
    where
      L: Lexer<'inp>,
      F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        + UnclosedEmitter<'inp, L, Lang>
        $($emitters)*,
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
        From<UnexpectedEot<L::Offset, Lang>> + crate::emitter::FromUnclosed<'inp, L, Lang>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: Default + ContainerT<O> + SeparatorHandler<'inp, L> + DelimiterHandler<'inp, L>,
      Delim: Delimiter<'inp, L, Lang>,
    {
      #[inline(always)]
      fn parse_input(
        &mut self,
        inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
      ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
        Wrapper(impl_separated_delim!(@map_self $depth self))
          .parse_input(inp)
          .map(|_| mem::take(&mut self.container))
      }
    }

    // Block 2: owned -> Spanned<Container>
    impl<'inp, L, F, Sep, O, Delim, Container, Ctx, Lang: ?Sized, Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>>
      ParseInput<'inp, L, Spanned<Container, L::Span>, Ctx, Lang, Cmpl>
      for With<Collect<$($owned)*, Container, Ctx, Lang, Cmpl>, PhantomSpan>
    where
      L: Lexer<'inp>,
      F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        + UnclosedEmitter<'inp, L, Lang>
        $($emitters)*,
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
        From<UnexpectedEot<L::Offset, Lang>> + crate::emitter::FromUnclosed<'inp, L, Lang>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: Default + ContainerT<O> + SeparatorHandler<'inp, L> + DelimiterHandler<'inp, L>,
      Delim: Delimiter<'inp, L, Lang>,
    {
      #[inline(always)]
      fn parse_input(
        &mut self,
        inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
      ) -> Result<Spanned<Container, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
        Wrapper(impl_separated_delim!(@map_primary $depth self))
          .parse_input(inp)
          .map(|span| Spanned::new(span, mem::take(&mut self.primary.container)))
      }
    }

    // Block 3: &mut ref -> L::Span
    impl<'inp, 'c, L, F, Sep, O, Delim, Container, Ctx, Lang: ?Sized, Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>>
      ParseInput<'inp, L, L::Span, Ctx, Lang, Cmpl>
      for Collect<&'c mut $($reft)*, &'c mut Container, Ctx, Lang, Cmpl>
    where
      L: Lexer<'inp>,
      F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        + UnclosedEmitter<'inp, L, Lang>
        $($emitters)*,
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
        From<UnexpectedEot<L::Offset, Lang>> + crate::emitter::FromUnclosed<'inp, L, Lang>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: ContainerT<O> + SeparatorHandler<'inp, L> + DelimiterHandler<'inp, L>,
      Delim: Delimiter<'inp, L, Lang>,
    {
      impl_separated_delim!(@inline $b3i
        fn parse_input(
          &mut self,
          input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
        ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
        where
          L: Lexer<'inp>,
          Ctx: ParseContext<'inp, L, Lang>,
        {
          impl_separated_delim!(@block3 $card [$($policy),*] self input)
        }
      );
    }

    struct Wrapper<T>(T);

    // Block 4: Wrapper -> L::Span
    impl<'inp, 'c, L, F, Sep, O, Delim, Container, Ctx, Lang: ?Sized, Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>>
      ParseInput<'inp, L, L::Span, Ctx, Lang, Cmpl>
      for Wrapper<Collect<$($wt)*, &'c mut Container, Ctx, Lang, Cmpl>>
    where
      L: Lexer<'inp>,
      F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        + UnclosedEmitter<'inp, L, Lang>
        $($emitters)*,
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
        From<UnexpectedEot<L::Offset, Lang>> + crate::emitter::FromUnclosed<'inp, L, Lang>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: ContainerT<O> + SeparatorHandler<'inp, L> + DelimiterHandler<'inp, L>,
      Delim: Delimiter<'inp, L, Lang>,
    {
      impl_separated_delim!(@inline $b4i
        fn parse_input(
          &mut self,
          inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
        ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
          impl_separated_delim!(@block4 $card [$($policy),*] self inp)
        }
      );
    }
  };
}

/// Generates 4 `ParseInput` impl blocks for `sep_while/parse/` leaf files.
macro_rules! impl_separated_while_parse {
  // ── @inline helper ───────────────────────────────────────────────────
  (@inline true $($item:tt)*) => { #[inline(always)] $($item)* };
  (@inline false $($item:tt)*) => { $($item)* };

  // ── @map_self: map_parser chain for block 1 ─────────────────────────
  (@map_self 0 $self:ident) => { $self.as_mut().map_parser(|p| p.as_mut()) };
  (@map_self 1 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_self 2 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_self 3 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };

  // ── @map_primary: map_parser chain for block 2 ──────────────────────
  (@map_primary 0 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.as_mut()) };
  (@map_primary 1 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_primary 2 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_primary 3 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };

  // ── @block3: block 3 body dispatch ──────────────────────────────────
  // depth=0, no policy
  (@block3 unbounded [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let (f, condition) = parser.parts_mut();
    let parser = Collect::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      &mut *container,
    );
    Wrapper(parser).parse_input($inp)
  }};
  (@block3 at_least [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let minimum = parser.minimum();
    let (f, condition) = parser.parser_mut().parts_mut();
    let parser = AtLeast::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      minimum.get(),
    );
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let maximum = parser.maximum();
    let (f, condition) = parser.parser_mut().parts_mut();
    let parser = AtMost::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
    );
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let maximum = parser.maximum();
    let minimum = parser.minimum();
    let (f, condition) = parser.parser_mut().parts_mut();
    let parser = Bounded::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
      minimum.get(),
    );
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // depth=1, single policy
  (@block3 unbounded [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let (f, condition) = parser.parser_mut().parts_mut();
    let parser = $p1::new(SeparatedWhile::new::<Sep>(&mut *f, &mut *condition));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut();
    let minimum = inner.minimum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = $p1::new(AtLeast::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      minimum.get(),
    ));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut();
    let maximum = inner.maximum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = $p1::new(AtMost::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
    ));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut();
    let maximum = inner.maximum();
    let minimum = inner.minimum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = $p1::new(Bounded::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
      minimum.get(),
    ));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // depth=2, double policy
  (@block3 unbounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let (f, condition) = parser.parser_mut().parser_mut().parts_mut();
    let parser = $p1::new($p2::new(SeparatedWhile::new::<Sep>(&mut *f, &mut *condition)));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut().parser_mut();
    let minimum = inner.minimum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = $p1::new($p2::new(AtLeast::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      minimum.get(),
    )));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut().parser_mut();
    let maximum = inner.maximum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = $p1::new($p2::new(AtMost::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
    )));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.parts_mut();
    let inner = parser.parser_mut().parser_mut();
    let maximum = inner.maximum();
    let minimum = inner.minimum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = $p1::new($p2::new(Bounded::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
      minimum.get(),
    )));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // ── @block4: block 4 body dispatch ──────────────────────────────────
  // depth=0, no policy
  (@block4 unbounded [] $self:ident $inp:ident) => {{
    const HANDLER: &Unbounded = &Unbounded;
    let (parser, container) = $self.0.parts_mut();
    parser.parse($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let minimum = parser.minimum();
    parser.parser_mut().parse($inp, container, &minimum, &minimum, &minimum)
  }};
  (@block4 at_most [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = parser.maximum();
    parser.parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 bounded [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = parser.to_with();
    parser.parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};

  // depth=1, single policy
  (@block4 unbounded [$p1:ident] $self:ident $inp:ident) => {{
    const HANDLER: &$p1<Unbounded> = &$p1::new(Unbounded);
    let (parser, container) = $self.0.parts_mut();
    parser.parser_mut().parse($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.minimum());
    parser.parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 at_most [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.maximum());
    parser.parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 bounded [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.to_with());
    parser.parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};

  // depth=2, double policy
  (@block4 unbounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    const HANDLER: &$p1<$p2<Unbounded>> = &$p1::new($p2::new(Unbounded));
    let (parser, container) = $self.0.parts_mut();
    parser.parser_mut().parser_mut().parse($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.minimum()));
    parser.parser_mut().parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 at_most [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.maximum()));
    parser.parser_mut().parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 bounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.to_with()));
    parser.parser_mut().parser_mut().parser_mut().parse($inp, container, &limitation, &limitation, &limitation)
  }};

  // ── Main entry point ────────────────────────────────────────────────
  (
    owned_type = [$($owned:tt)*],
    ref_type = [$($reft:tt)*],
    wrapper_type = [$($wt:tt)*],
    map_depth = $depth:tt,
    cardinality = $card:ident,
    policy = [$($policy:ident),*],
    emitters = {$($emitters:tt)*},
    block3_inline = $b3i:ident,
    block4_inline = $b4i:ident $(,)?
  ) => {
    // Block 1: owned -> Container
    impl<'inp, L, F, Sep, Condition, O, Container, Ctx, Lang: ?Sized, W>
      ParseInput<'inp, L, Container, Ctx, Lang>
      for Collect<$($owned)*, Container, Ctx, Lang>
    where
      L: Lexer<'inp>,
      F: ParseInput<'inp, L, O, Ctx, Lang>,
      Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        $($emitters)*,
      // The separator-slot decision gate surfaces a terminal scanner stop as this end-of-input
      // error (`try_expect_or_stop`); the plain separated families carry the bound the delimited
      // ones already required.
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: Default + ContainerT<O> + SeparatorHandler<'inp, L>,
      W: Window,
    {
      #[inline(always)]
      fn parse_input(
        &mut self,
        inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
      ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
        Wrapper(impl_separated_while_parse!(@map_self $depth self))
          .parse_input(inp)
          .map(|_| mem::take(&mut self.container))
      }
    }

    // Block 2: owned -> Spanned<Container>
    impl<'inp, L, F, Sep, Condition, O, Container, Ctx, Lang: ?Sized, W>
      ParseInput<'inp, L, Spanned<Container, L::Span>, Ctx, Lang>
      for With<Collect<$($owned)*, Container, Ctx, Lang>, PhantomSpan>
    where
      L: Lexer<'inp>,
      F: ParseInput<'inp, L, O, Ctx, Lang>,
      Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        $($emitters)*,
      // The separator-slot decision gate surfaces a terminal scanner stop as this end-of-input
      // error (`try_expect_or_stop`); the plain separated families carry the bound the delimited
      // ones already required.
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: Default + ContainerT<O> + SeparatorHandler<'inp, L>,
      W: Window,
    {
      #[inline(always)]
      fn parse_input(
        &mut self,
        inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
      ) -> Result<Spanned<Container, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
        Wrapper(impl_separated_while_parse!(@map_primary $depth self))
          .parse_input(inp)
          .map(|span| Spanned::new(span, mem::take(&mut self.primary.container)))
      }
    }

    // Block 3: &mut ref -> L::Span
    impl<'inp, 'c, L, F, Sep, Condition, O, Container, Ctx, Lang: ?Sized, W>
      ParseInput<'inp, L, L::Span, Ctx, Lang>
      for Collect<&'c mut $($reft)*, &'c mut Container, Ctx, Lang>
    where
      L: Lexer<'inp>,
      F: ParseInput<'inp, L, O, Ctx, Lang>,
      Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        $($emitters)*,
      // The separator-slot decision gate surfaces a terminal scanner stop as this end-of-input
      // error (`try_expect_or_stop`); the plain separated families carry the bound the delimited
      // ones already required.
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: ContainerT<O> + SeparatorHandler<'inp, L>,
      W: Window,
    {
      impl_separated_while_parse!(@inline $b3i
        fn parse_input(
          &mut self,
          input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
        ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
        where
          L: Lexer<'inp>,
          Ctx: ParseContext<'inp, L, Lang>,
        {
          impl_separated_while_parse!(@block3 $card [$($policy),*] self input)
        }
      );
    }

    struct Wrapper<T>(T);

    // Block 4: Wrapper -> L::Span
    impl<'inp, 'c, L, F, Sep, Condition, O, Container, Ctx, Lang: ?Sized, W>
      ParseInput<'inp, L, L::Span, Ctx, Lang>
      for Wrapper<Collect<$($wt)*, &'c mut Container, Ctx, Lang>>
    where
      L: Lexer<'inp>,
      F: ParseInput<'inp, L, O, Ctx, Lang>,
      Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        $($emitters)*,
      // The separator-slot decision gate surfaces a terminal scanner stop as this end-of-input
      // error (`try_expect_or_stop`); the plain separated families carry the bound the delimited
      // ones already required.
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: ContainerT<O> + SeparatorHandler<'inp, L>,
      W: Window,
    {
      impl_separated_while_parse!(@inline $b4i
        fn parse_input(
          &mut self,
          inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
        ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
          impl_separated_while_parse!(@block4 $card [$($policy),*] self inp)
        }
      );
    }
  };
}

/// Generates 4 `ParseInput` impl blocks for `sep_while/delim/` leaf files.
macro_rules! impl_separated_while_delim {
  // ── @inline helper ───────────────────────────────────────────────────
  (@inline true $($item:tt)*) => { #[inline(always)] $($item)* };
  (@inline false $($item:tt)*) => { $($item)* };

  // ── @map_self: map_parser chain for block 1 ─────────────────────────
  (@map_self 1 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_self 2 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_self 3 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };
  (@map_self 4 $self:ident) => { $self.as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))))) };

  // ── @map_primary: map_parser chain for block 2 ──────────────────────
  (@map_primary 1 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_primary 2 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_primary 3 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };
  (@map_primary 4 $self:ident) => { $self.primary_mut().as_mut().map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))))) };

  // ── @block3: block 3 body dispatch ──────────────────────────────────
  // depth=0, no policy
  (@block3 unbounded [] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let (f, condition) = delim.parser.parts_mut();
    let parser = DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut *condition));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let minimum = delim.parser.minimum();
    let (f, condition) = delim.parser.parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new(AtLeast::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      minimum.get(),
    ));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let maximum = delim.parser.maximum();
    let (f, condition) = delim.parser.parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new(AtMost::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
    ));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let maximum = delim.parser.maximum();
    let minimum = delim.parser.minimum();
    let (f, condition) = delim.parser.parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new(Bounded::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
      minimum.get(),
    ));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // depth=1, single policy
  (@block3 unbounded [$p1:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let (f, condition) = delim.parser.parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new(SeparatedWhile::new::<Sep>(&mut **f, &mut *condition)));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [$p1:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut();
    let minimum = inner.minimum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new(AtLeast::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      minimum.get(),
    )));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [$p1:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut();
    let maximum = inner.maximum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new(AtMost::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
    )));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [$p1:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut();
    let maximum = inner.maximum();
    let minimum = inner.minimum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new(Bounded::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
      minimum.get(),
    )));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // depth=2, double policy
  (@block3 unbounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let (f, condition) = delim.parser.parser_mut().parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new($p2::new(SeparatedWhile::new::<Sep>(&mut **f, &mut *condition))));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_least [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut().parser_mut();
    let minimum = inner.minimum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new($p2::new(AtLeast::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      minimum.get(),
    ))));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 at_most [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut().parser_mut();
    let maximum = inner.maximum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new($p2::new(AtMost::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
    ))));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};
  (@block3 bounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (delim, container) = $self.parts_mut();
    let inner = delim.parser.parser_mut().parser_mut();
    let maximum = inner.maximum();
    let minimum = inner.minimum();
    let (f, condition) = inner.parser_mut().parts_mut();
    let parser = DelimitedBy::<_, Delim>::new($p1::new($p2::new(Bounded::new(
      SeparatedWhile::new::<Sep>(&mut **f, &mut *condition),
      maximum.get(),
      minimum.get(),
    ))));
    Wrapper(Collect::new(parser, &mut **container)).parse_input($inp)
  }};

  // ── @block4: block 4 body dispatch ──────────────────────────────────
  // depth=0, no policy
  (@block4 unbounded [] $self:ident $inp:ident) => {{
    const HANDLER: &Unbounded = &Unbounded;
    let (parser, container) = $self.0.parts_mut();
    let (f, condition) = parser.parser.parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let minimum = parser.parser.minimum();
    let (f, condition) = parser.parser.parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, &minimum, &minimum, &minimum)
  }};
  (@block4 at_most [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let maximum = parser.parser.maximum();
    let (f, condition) = parser.parser.parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, &maximum, &maximum, &maximum)
  }};
  (@block4 bounded [] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = parser.parser.to_with();
    let (f, condition) = parser.parser.parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};

  // depth=1, single policy
  (@block4 unbounded [$p1:ident] $self:ident $inp:ident) => {{
    const HANDLER: &$p1<Unbounded> = &$p1::new(Unbounded);
    let (parser, container) = $self.0.parts_mut();
    let (f, condition) = parser.parser.parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.parser.minimum());
    let (f, condition) = parser.parser.parser_mut().parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 at_most [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.parser.maximum());
    let (f, condition) = parser.parser.parser_mut().parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 bounded [$p1:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new(parser.parser.parser.to_with());
    let (f, condition) = parser.parser.parser_mut().parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};

  // depth=2, double policy
  (@block4 unbounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    const HANDLER: &$p1<$p2<Unbounded>> = &$p1::new($p2::new(Unbounded));
    let (parser, container) = $self.0.parts_mut();
    let (f, condition) = parser.parser.parser_mut().parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, HANDLER, HANDLER, HANDLER)
  }};
  (@block4 at_least [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.parser.minimum()));
    let (f, condition) = parser.parser.parser_mut().parser_mut().parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 at_most [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.parser.maximum()));
    let (f, condition) = parser.parser.parser_mut().parser_mut().parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};
  (@block4 bounded [$p1:ident, $p2:ident] $self:ident $inp:ident) => {{
    let (parser, container) = $self.0.parts_mut();
    let limitation = $p1::new($p2::new(parser.parser.parser.parser.to_with()));
    let (f, condition) = parser.parser.parser_mut().parser_mut().parser_mut().parts_mut();
    DelimitedBy::<_, Delim>::new(SeparatedWhile::new::<Sep>(&mut **f, &mut **condition))
      .parse_separated($inp, container, &limitation, &limitation, &limitation)
  }};

  // ── Main entry point ────────────────────────────────────────────────
  (
    owned_type = [$($owned:tt)*],
    ref_type = [$($reft:tt)*],
    wrapper_type = [$($wt:tt)*],
    map_depth = $depth:tt,
    cardinality = $card:ident,
    policy = [$($policy:ident),*],
    emitters = {$($emitters:tt)*},
    block3_inline = $b3i:ident,
    block4_inline = $b4i:ident $(,)?
  ) => {
    // Block 1: owned -> Container
    impl<'inp, L, F, Sep, Condition, O, Delim, Container, Ctx, Lang: ?Sized, W>
      ParseInput<'inp, L, Container, Ctx, Lang>
      for Collect<$($owned)*, Container, Ctx, Lang>
    where
      L: Lexer<'inp>,
      F: ParseInput<'inp, L, O, Ctx, Lang>,
      Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        + UnclosedEmitter<'inp, L, Lang>
        $($emitters)*,
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
        From<UnexpectedEot<L::Offset, Lang>> + crate::emitter::FromUnclosed<'inp, L, Lang>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: Default + ContainerT<O> + SeparatorHandler<'inp, L> + DelimiterHandler<'inp, L>,
      Delim: Delimiter<'inp, L, Lang>,
      W: Window,
    {
      #[inline(always)]
      fn parse_input(
        &mut self,
        inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
      ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
        Wrapper(impl_separated_while_delim!(@map_self $depth self))
          .parse_input(inp)
          .map(|_| mem::take(&mut self.container))
      }
    }

    // Block 2: owned -> Spanned<Container>
    impl<'inp, L, F, Sep, Condition, O, Delim, Container, Ctx, Lang: ?Sized, W>
      ParseInput<'inp, L, Spanned<Container, L::Span>, Ctx, Lang>
      for With<Collect<$($owned)*, Container, Ctx, Lang>, PhantomSpan>
    where
      L: Lexer<'inp>,
      F: ParseInput<'inp, L, O, Ctx, Lang>,
      Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        + UnclosedEmitter<'inp, L, Lang>
        $($emitters)*,
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
        From<UnexpectedEot<L::Offset, Lang>> + crate::emitter::FromUnclosed<'inp, L, Lang>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: Default + ContainerT<O> + SeparatorHandler<'inp, L> + DelimiterHandler<'inp, L>,
      Delim: Delimiter<'inp, L, Lang>,
      W: Window,
    {
      #[inline(always)]
      fn parse_input(
        &mut self,
        inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
      ) -> Result<Spanned<Container, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
        Wrapper(impl_separated_while_delim!(@map_primary $depth self))
          .parse_input(inp)
          .map(|span| Spanned::new(span, mem::take(&mut self.primary.container)))
      }
    }

    // Block 3: &mut ref -> L::Span
    impl<'inp, 'c, L, F, Sep, Condition, O, Delim, Container, Ctx, Lang: ?Sized, W>
      ParseInput<'inp, L, L::Span, Ctx, Lang>
      for Collect<&'c mut $($reft)*, &'c mut Container, Ctx, Lang>
    where
      L: Lexer<'inp>,
      F: ParseInput<'inp, L, O, Ctx, Lang>,
      Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        + UnclosedEmitter<'inp, L, Lang>
        $($emitters)*,
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
        From<UnexpectedEot<L::Offset, Lang>> + crate::emitter::FromUnclosed<'inp, L, Lang>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: ContainerT<O> + SeparatorHandler<'inp, L> + DelimiterHandler<'inp, L>,
      Delim: Delimiter<'inp, L, Lang>,
      W: Window,
    {
      impl_separated_while_delim!(@inline $b3i
        fn parse_input(
          &mut self,
          input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
        ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
        where
          L: Lexer<'inp>,
          Ctx: ParseContext<'inp, L, Lang>,
        {
          impl_separated_while_delim!(@block3 $card [$($policy),*] self input)
        }
      );
    }

    struct Wrapper<T>(T);

    // Block 4: Wrapper -> L::Span
    impl<'inp, 'c, L, F, Sep, Condition, O, Delim, Container, Ctx, Lang: ?Sized, W>
      ParseInput<'inp, L, L::Span, Ctx, Lang>
      for Wrapper<Collect<$($wt)*, &'c mut Container, Ctx, Lang>>
    where
      L: Lexer<'inp>,
      F: ParseInput<'inp, L, O, Ctx, Lang>,
      Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
      Sep: Punctuator<'inp, L, Lang>,
      Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
        + FullContainerEmitter<'inp, L, Lang>
        + UnclosedEmitter<'inp, L, Lang>
        $($emitters)*,
      <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
        From<UnexpectedEot<L::Offset, Lang>> + crate::emitter::FromUnclosed<'inp, L, Lang>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: ContainerT<O> + SeparatorHandler<'inp, L> + DelimiterHandler<'inp, L>,
      Delim: Delimiter<'inp, L, Lang>,
      W: Window,
    {
      impl_separated_while_delim!(@inline $b4i
        fn parse_input(
          &mut self,
          inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
        ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
          impl_separated_while_delim!(@block4 $card [$($policy),*] self inp)
        }
      );
    }
  };
}
