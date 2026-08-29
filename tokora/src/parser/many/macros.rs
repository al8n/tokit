/// The try-driven element cycle, stated once for the two drivers that run it: `Repeated::parse`
/// and `delim/repeated.rs`'s `parse_repeated`.
///
/// Expands to a `loop` **expression** whose value is the trip baseline of the attempt that
/// concluded absence — the value each driver's epilogue hands to `many::absence_after_element`.
///
/// # What the cycle is, and what it is not
///
/// Descent baseline, element attempt, the admission arm, the failure-chokepoint arm, the stall
/// test, the bookkeeping. Nothing else: no absence gate, no close probe, no stop hook. The plain
/// driver's decline is a bare `break trips`; the delimited driver's is its whole four-way close
/// probe — both arrive through `on_decline:` as the caller's own tokens, so `many::close_after_element`
/// stays reachable from delimited driver text alone and this macro contains no close, absence or
/// stop code at all. That is stage 2's verdict turned into structure: the delimited try driver is
/// the plain cycle under an opener, with the close position in the decline hole, and nothing else.
///
/// # Why this is a macro and not a function, and what that costs to keep
///
/// [#259](https://github.com/al8n/tokora/issues/259) asks for a shared engine, and the
/// function-shaped one was measured. Extracting this loop into a `drive_elements` method preserved
/// behaviour and moved three witnessed instruction-count rows — `at_least` +2.383%,
/// `at_most_shallow` +1.214% (aarch64) / +1.030% (x86_64), `at_most` +0.535% — while nine other
/// rows read +0.000%. The cost is not a per-element tax the signature introduces: `#[inline(always)]`
/// on the extracted function produced a byte-identical binary, `repeated` and `at_least`
/// monomorphize the *same* extracted loop with the same no-op hook and read +0.000% and +2.383%
/// respectively, the `at_most` cost decomposes as ~15–18 instructions per COLLECTION rather than
/// per element, and a different LLVM makes the same function *smaller*. It is a codegen re-roll —
/// a whole-function register-allocation re-solve seeded by the new boundary — so it lands
/// unpredictably, per monomorphization and per ISA, and its best case is a tie with what this macro
/// guarantees.
///
/// A `macro_rules!` expansion presents the same tokens the driver spelled before. Same tokens, same
/// MIR, same object code, same instructions executed: exactly +0.000% on every row, on both ISAs
/// and any toolchain, without measuring them. That guarantee is the whole reason for this shape,
/// and it holds only while both expansions stay token-identical to the text they replaced — which
/// is what the constraints below protect.
///
/// * **This macro declares nothing of the caller's state and moves no declaration.** Every local
///   keeps its name, its position and its initializer in the driver, and arrives as an `ident`.
///   The two drivers initialize their error anchor differently — `anchor.clone()` in the plain one,
///   `inp.cursor().clone()` in the delimited one. Those are semantically equal at that point and
///   token-different, and unifying them is one of the re-roll's suspects, so each initializer stays
///   where it is.
/// * **No declaration moves relative to any other, in either driver.** MIR is scheduled as
///   written, so reordering two pure snapshot reads is semantically inert and still re-rolls.
/// * **`Ok(Decline) => $decline` takes each driver's arm verbatim.** In particular the delimited
///   driver's two close-probe sites are *not* unified into one epilogue: that is a separate,
///   unmeasured restructure, deliberately not taken here.
/// * **Hygiene.** `$trips` is the caller's ident, so `let $trips = …` declares the caller's binding
///   and the caller's `on_decline` tokens can name it — the same dance this file documents for
///   `self` in the `impl_separated_*` macros. `break` inside `$decline` binds to the `loop` here
///   regardless of hygiene, and `?`/`return` inside it leave the driver function. `Accept`,
///   `Decline`, `admit_element` and `file_element_failure` resolve at the expansion site against
///   each driver's own imports, exactly as every other body in this file does.
///
/// The static check that keeps all of that honest is a text-section comparison, not a benchmark:
/// build `tokora-icount --profile bench` on either side and diff every symbol's disassembly with
/// addresses normalized. Zero differing symbols is the acceptance, and any differing symbol names
/// the exact token that leaked.
macro_rules! try_element_cycle {
  (
    $inp:ident, $f:expr, $counts:expr, $container:ident,
    anchor: $anchor:ident,
    error_anchor: $cursor:ident,
    committed: $committed:ident,
    full: $full:ident,
    nums: $nums:ident,
    scans: $scans:ident,
    trips: $trips:ident,
    on_decline: $decline:expr $(,)?
  ) => {
    loop {
      // The descent witness's baseline, taken once per ELEMENT — the attempt the chokepoint below
      // judges, and the one each driver's absence exits judge too. `many::file_element_failure`
      // says why it belongs here and not out beside `latch`.
      let $trips = $inp.trip_snapshot();
      match $f.try_parse_input($inp) {
        // One admission, and it settles this element's count bound before the destination is
        // offered the element — see `many::admit_element` for why that order is the function's
        // rather than this cycle's.
        // TODO(al8n): tracing dropped element
        Ok(Accept(item)) => admit_element($counts, &mut $nums, &mut $full, $container, item, $inp, &$anchor)?,
        // No more elements. The plain driver breaks with the baseline; the delimited one probes
        // the close position here, which is the delimiter's entire contribution to this cycle.
        Ok(Decline) => $decline,
        // File the failure as a diagnostic and keep looping — unless it is one of the three the
        // never-recoverable law forbids spending, in which case re-raise it untouched. The gate is
        // the chokepoint's, not this cycle's: see `many::file_element_failure` for the three
        // witnesses and for why `trips` is taken per ELEMENT rather than per collection. `?` here
        // propagates both a re-raise and an emitter that refused the diagnostic, exactly as the
        // hand-written arms this replaced did.
        Err(err) => file_element_failure($inp, err, &$cursor, $scans, $trips)?,
      }

      // A cycle that consumed nothing re-sees the same input and would retry forever. The progress
      // metric is committed consumption (`span().end()`), never the cache-front cursor — a lookahead
      // fill (a `probe_close` or `try_expect` decline pushing a token back) moves that across skipped
      // trivia without consuming, reading a zero-width element as false progress. `<=`, not `==`: the
      // watermark cannot regress within a cycle, so anything not strictly ahead is a stall. The
      // error-span anchor stays whichever cursor the driver named.
      let new_committed = $inp.span().end();
      if new_committed <= $committed {
        break $trips;
      }
      $committed = new_committed;
      $cursor = $inp.cursor().clone();
    }
  };
}

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
/// Instead, blocks 1+2 use depth-based variant dispatch (`@map_collect`),
/// and blocks 3+4 use dispatch by `(cardinality, [policy_types])` (`@block3`/`@block4`).
macro_rules! impl_separated_parse {
  // ── @inline helper ───────────────────────────────────────────────────
  (@inline true $($item:tt)*) => { #[inline(always)] $($item)* };
  (@inline false $($item:tt)*) => { $($item)* };

  // ── @map_collect: map_parser chain for blocks 1 and 2 ───────────────
  //
  // The collector arrives already split from its owner by `Collect::attempt`, so both blocks
  // start from the attempt's borrow rather than from `self.as_mut()`: the container an owning
  // collection parses into is never the one that lives in the parser object.
  (@map_collect 0 $c:ident) => { $c.map_parser(|p| p.as_mut()) };
  (@map_collect 1 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_collect 2 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_collect 3 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };

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
        self
          .attempt(|c| Wrapper(impl_separated_parse!(@map_collect $depth c)).parse_input(inp))
          .map(|(_, collected)| collected)
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
        self
          .primary_mut()
          .attempt(|c| Wrapper(impl_separated_parse!(@map_collect $depth c)).parse_input(inp))
          .map(|(span, collected)| Spanned::new(span, collected))
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

  // ── @map_collect: map_parser chain for blocks 1 and 2 ───────────────
  //
  // The collector arrives already split from its owner by `Collect::attempt`, so both blocks
  // start from the attempt's borrow rather than from `self.as_mut()`: the container an owning
  // collection parses into is never the one that lives in the parser object.
  (@map_collect 1 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_collect 2 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_collect 3 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };
  (@map_collect 4 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))))) };

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
        From<UnexpectedEot<L::Offset, Lang>>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: Default + ContainerT<O> + SeparatorHandler<'inp, L> + DelimiterHandler<'inp, L>,
      Delim: Delimiter<'inp, L, Lang>,
    {
      #[inline(always)]
      fn parse_input(
        &mut self,
        inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
      ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
        self
          .attempt(|c| Wrapper(impl_separated_delim!(@map_collect $depth c)).parse_input(inp))
          .map(|(_, collected)| collected)
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
        From<UnexpectedEot<L::Offset, Lang>>,
      Ctx: ParseContext<'inp, L, Lang>,
      Container: Default + ContainerT<O> + SeparatorHandler<'inp, L> + DelimiterHandler<'inp, L>,
      Delim: Delimiter<'inp, L, Lang>,
    {
      #[inline(always)]
      fn parse_input(
        &mut self,
        inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
      ) -> Result<Spanned<Container, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
        self
          .primary_mut()
          .attempt(|c| Wrapper(impl_separated_delim!(@map_collect $depth c)).parse_input(inp))
          .map(|(span, collected)| Spanned::new(span, collected))
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
        From<UnexpectedEot<L::Offset, Lang>>,
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
        From<UnexpectedEot<L::Offset, Lang>>,
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

  // ── @map_collect: map_parser chain for blocks 1 and 2 ───────────────
  //
  // The collector arrives already split from its owner by `Collect::attempt`, so both blocks
  // start from the attempt's borrow rather than from `self.as_mut()`: the container an owning
  // collection parses into is never the one that lives in the parser object.
  (@map_collect 0 $c:ident) => { $c.map_parser(|p| p.as_mut()) };
  (@map_collect 1 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_collect 2 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_collect 3 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };

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
        self
          .attempt(|c| Wrapper(impl_separated_while_parse!(@map_collect $depth c)).parse_input(inp))
          .map(|(_, collected)| collected)
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
        self
          .primary_mut()
          .attempt(|c| Wrapper(impl_separated_while_parse!(@map_collect $depth c)).parse_input(inp))
          .map(|(span, collected)| Spanned::new(span, collected))
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

  // ── @map_collect: map_parser chain for blocks 1 and 2 ───────────────
  //
  // The collector arrives already split from its owner by `Collect::attempt`, so both blocks
  // start from the attempt's borrow rather than from `self.as_mut()`: the container an owning
  // collection parses into is never the one that lives in the parser object.
  (@map_collect 1 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.as_mut())) };
  (@map_collect 2 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))) };
  (@map_collect 3 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut())))) };
  (@map_collect 4 $c:ident) => { $c.map_parser(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.map_parser_mut(|p| p.as_mut()))))) };

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
        From<UnexpectedEot<L::Offset, Lang>>,
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
        self
          .attempt(|c| Wrapper(impl_separated_while_delim!(@map_collect $depth c)).parse_input(inp))
          .map(|(_, collected)| collected)
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
        From<UnexpectedEot<L::Offset, Lang>>,
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
        self
          .primary_mut()
          .attempt(|c| Wrapper(impl_separated_while_delim!(@map_collect $depth c)).parse_input(inp))
          .map(|(span, collected)| Spanned::new(span, collected))
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
        From<UnexpectedEot<L::Offset, Lang>>,
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
        From<UnexpectedEot<L::Offset, Lang>>,
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
