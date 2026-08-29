use core::marker::PhantomData;

use crate::input::Complete;

/// A parser that collects results into a container.
///
/// # The three destinations, on every repetition driver
///
/// A collection can be asked for its container, for that container paired with the construct's
/// span, or — when the caller owns the storage — for the span alone:
///
/// * `Collect<P, Container>` parsing to `Container`, the owning form;
/// * [`With<Collect<P, Container>, PhantomSpan>`](crate::parser::With) parsing to
///   [`Spanned<Container, L::Span>`](crate::span::Spanned), the same transfer with the span
///   beside it;
/// * `Collect<&mut P, &mut Container>` parsing to `L::Span`, the **borrowed** form. It is the only
///   one that leaves the caller holding the container on the **failure** arm, so it is how a
///   partially-parsed collection is inspected after an error.
///
/// # Why the spanned destination wears a wrapper
///
/// A destination is selected by the **output** parameter of
/// [`ParseInput`](crate::ParseInput), so two destinations written onto one `Self` type are two
/// impls that differ in nothing a caller can point at. A caller who leaves that parameter to
/// inference — which every `.spanned()`-shaped extension method does, because the output is a
/// parameter of such a trait and never appears in the method's return type — then gets
/// `error[E0283]: type annotations needed` instead of a parser, and no annotation at the call
/// site can help, since the ambiguity is in selecting the method rather than in typing its
/// result. Only a downstream caller sees it: an uninstantiated generic instantiates no
/// ambiguity, and a test that pins the output through an annotated return type has resolved the
/// choice before it is made.
///
/// So `Collect<P, Container>` carries exactly **one** output, `Collect<&mut P, &mut Container>`
/// exactly one, and the spanned destination lives on
/// [`With<.., PhantomSpan>`](crate::parser::With) — a distinct `Self`, chosen syntactically. This
/// holds on all twelve repetition drivers.
/// [`PhantomSpan`](crate::utils::marker::PhantomSpan) carries no data; it is the type that names
/// the destination.
///
/// All three are available on every repetition driver — plain and separated, `while` and not,
/// delimited and not. The separated families add a fourth arrangement for their separator
/// policies. `tokora/tests/repetition_behavioural_matrix.rs`'s
/// `the_owning_and_borrowed_contracts_collect_the_same_elements` holds the owning and borrowed
/// forms to the same collection over every driver, and
/// `a_failed_borrowed_attempt_holds_only_what_it_parsed` is what the borrowed one exists for.
///
/// # Where the input rests, and what the reported span therefore covers
///
/// The span this combinator hands back — directly, as the `L::Span` of a borrowed collection, and
/// inside the `Spanned<Container, L::Span>` of an owning one — is built with
/// [`InputRef::span_since`](crate::InputRef::span_since), which reads
/// [`InputRef::cursor`](crate::InputRef::cursor): the **lookahead front**, not the committed
/// watermark.
///
/// A repetition construct learns that its elements have run out by *looking* at the token that
/// ends them, and a declined peek leaves that token in the lookahead cache. So the reported end is
/// the **start of the next token**, trailing trivia included, whenever the construct stopped that
/// way, and the end of the last token consumed when it looked no further — at end of input, or
/// after committing a closing delimiter, or on the failure arm. Over `1 2 3    +` the collection
/// ends at the `3` and the reported span ends at the `+`, four bytes later.
///
/// This is uniform across every repetition driver: plain and separated, `while` and not,
/// delimited and not, all rest in the same place, so a span does not change with the combinator
/// that produced it. `tokora/tests/repetition_behavioural_matrix.rs`'s
/// `a_construct_rests_at_the_lookahead_front` holds it over all forty-eight of them. A caller that
/// needs an end past no trivia wants [`InputRef::span`](crate::InputRef::span)'s end instead — the
/// committed watermark, which is the end of the last token the parse actually took.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Collect<P, Container, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  pub(crate) parser: P,
  pub(crate) container: Container,
  pub(crate) _ctx: PhantomData<Ctx>,
  pub(crate) _lang: PhantomData<Lang>,
  pub(crate) _cmpl: PhantomData<Cmpl>,
}

impl<P, Container, Ctx, Lang: ?Sized, Cmpl> Collect<P, Container, Ctx, Lang, Cmpl> {
  /// Creates a new `Collect` combinator.
  #[inline(always)]
  pub const fn new(parser: P, container: Container) -> Self {
    Self {
      parser,
      container,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }

  /// Creates a mutable reference version of this `Collect` combinator.
  #[inline(always)]
  pub const fn as_mut(&mut self) -> Collect<&mut P, &mut Container, Ctx, Lang, Cmpl> {
    Collect {
      parser: &mut self.parser,
      container: &mut self.container,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }

  /// Maps the inner parser to a new parser.
  #[inline(always)]
  pub fn map_parser<F, P2>(self, f: F) -> Collect<P2, Container, Ctx, Lang, Cmpl>
  where
    F: FnOnce(P) -> P2,
  {
    Collect {
      parser: f(self.parser),
      container: self.container,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }

  /// Returns mutable references to the inner parser and container.
  #[inline(always)]
  pub fn parts_mut(&mut self) -> (&mut P, &mut Container) {
    (&mut self.parser, &mut self.container)
  }

  /// Maps the inner container to a new container.
  #[inline(always)]
  pub fn map_container<F, C2>(self, f: F) -> Collect<P, C2, Ctx, Lang, Cmpl>
  where
    F: FnOnce(Container) -> C2,
  {
    Collect {
      parser: self.parser,
      container: f(self.container),
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

/// The owning-collection transfer, whose only consumers are the repetition drivers.
#[cfg(feature = "many")]
impl<P, Container, Ctx, Lang: ?Sized, Cmpl> Collect<P, Container, Ctx, Lang, Cmpl>
where
  Container: Default,
{
  /// Runs one **owning** collection attempt against storage detached from `self`, and hands back
  /// what the attempt collected.
  ///
  /// # Why the storage has to move first
  ///
  /// An owning collector's container is parser-internal state on the way to becoming the return
  /// value. The transfer used to be `parse(..).map(|_| mem::take(&mut self.container))`, which
  /// runs on the success arm only: an attempt that accepted elements and *then* failed left them
  /// in `self`, so the next use of the same parser appended to the previous failure's residue and
  /// could return values the caller never fed it. A long-lived parser reused across inputs mixed
  /// them.
  ///
  /// Moving the storage out **before** the attempt makes the isolation total rather than
  /// arm-by-arm. `self.container` is `Default::default()` for the whole attempt, so:
  ///
  /// * success hands back the attempt's own storage;
  /// * `Err` drops it here, with the error;
  /// * a **panic** drops it by ordinary unwinding, so a host that catches one can reuse the
  ///   parser — the arm-by-arm form could not express that at all.
  ///
  /// A seed installed by `collect_with` is the first attempt's storage and shares its fate: a
  /// failed attempt drops the seed instead of leaving it, plus whatever partial elements were
  /// collected on top of it, as the next attempt's starting point.
  ///
  /// This is the only owning transfer in the crate; borrowed `Collect<_, &mut Container, _>` is a
  /// different contract, where the caller owns the container and observes it deliberately, and it
  /// is untouched.
  ///
  /// The closure's two borrows share **one** lifetime, because the driving `ParseInput` impls are
  /// written for `Collect<&'c mut P, &'c mut Container, …>` and `&mut` is invariant in what it
  /// points at: two independently quantified lifetimes would leave no `'c` for them to be.
  #[inline(always)]
  pub(crate) fn attempt<T, E>(
    &mut self,
    f: impl for<'a> FnOnce(Collect<&'a mut P, &'a mut Container, Ctx, Lang, Cmpl>) -> Result<T, E>,
  ) -> Result<(T, Container), E> {
    let mut collected = core::mem::take(&mut self.container);
    let out = f(Collect::new(&mut self.parser, &mut collected));
    out.map(|value| (value, collected))
  }
}
