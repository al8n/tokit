use super::*;

/// A parser that matches its inner parser at least `minimum` times.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AtLeast<P> {
  pub(in crate::parser) minimum: Minimum,
  pub(in crate::parser) parser: P,
}

impl<P> AtLeast<P> {
  /// Creates a new `AtLeast` parser that matches its inner parser at least `minimum` times.
  #[inline(always)]
  pub const fn new(parser: P, minimum: usize) -> Self {
    Self {
      minimum: Minimum::new(minimum),
      parser,
    }
  }

  /// Returns the minimum number of times the inner parser should match.
  #[inline(always)]
  pub const fn minimum(&self) -> Minimum {
    self.minimum
  }

  /// Creates a `Bounded` parser that matches its inner parser at least `minimum` and at most `maximum` times.
  #[inline(always)]
  pub fn at_most(self, maximum: usize) -> Bounded<P>
  where
    Self: Apply<Bounded<P>, Options = Maximum>,
  {
    self.apply(Maximum::new(maximum))
  }

  define_many_delimited_methods!();

  /// Returns a mutable reference to the inner parser.
  #[inline(always)]
  pub const fn parser_mut(&mut self) -> &mut P {
    &mut self.parser
  }

  /// Consumes the parser, returning the inner parser.
  #[inline(always)]
  pub fn into_parser(self) -> P {
    self.parser
  }

  /// Maps the inner parser to a new parser using the given function.
  #[inline(always)]
  pub fn map_parser_mut<'a, F, NP>(&'a mut self, f: F) -> AtLeast<NP>
  where
    F: FnOnce(&'a mut P) -> NP,
    NP: 'a,
  {
    AtLeast {
      minimum: self.minimum,
      parser: f(&mut self.parser),
    }
  }
}
