use core::marker::PhantomData;

use crate::input::Complete;

/// A parser that collects results into a container.
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
