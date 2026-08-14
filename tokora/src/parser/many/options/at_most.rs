use super::*;

/// A parser that matches its inner parser at most `maximum` times.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AtMost<P> {
  pub(in crate::parser) maximum: Maximum,
  pub(in crate::parser) parser: P,
}

impl<P> AtMost<P> {
  /// Creates a new `AtMost` parser that matches its inner parser at most `maximum` times.
  #[inline(always)]
  pub const fn new(parser: P, maximum: usize) -> Self {
    Self {
      maximum: Maximum::new(maximum),
      parser,
    }
  }

  /// Creates a `Bounded` parser that matches its inner parser at least `minimum` and at most `maximum` times.
  #[inline(always)]
  pub fn at_least(self, minimum: usize) -> Bounded<P>
  where
    Self: Apply<Bounded<P>, Options = Minimum>,
  {
    self.apply(Minimum::new(minimum))
  }

  /// Returns the maximum number of times the inner parser should match.
  #[inline(always)]
  pub const fn maximum(&self) -> Maximum {
    self.maximum
  }

  define_many_delimited_methods!();

  /// Returns a mutable reference to the inner parser.
  #[inline(always)]
  pub const fn parser_mut(&mut self) -> &mut P {
    &mut self.parser
  }

  /// Maps the inner parser to a new parser using the given function.
  #[inline(always)]
  pub fn map_parser_mut<'a, F, NP>(&'a mut self, f: F) -> AtMost<NP>
  where
    F: FnOnce(&'a mut P) -> NP,
    NP: 'a,
  {
    AtMost {
      maximum: self.maximum,
      parser: f(&mut self.parser),
    }
  }
}
