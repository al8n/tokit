use super::*;

/// A parser that matches its inner parser between `minimum` and `maximum` times.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bounded<P> {
  pub(in crate::parser) maximum: Maximum,
  pub(in crate::parser) minimum: Minimum,
  pub(in crate::parser) parser: P,
}

impl<P> Bounded<P> {
  /// Creates a new `Bounded` parser that matches its inner parser between `minimum` and `maximum` times.
  #[inline(always)]
  pub const fn new(parser: P, maximum: usize, minimum: usize) -> Self {
    Self {
      maximum: Maximum::new(maximum),
      minimum: Minimum::new(minimum),
      parser,
    }
  }

  /// Returns the maximum number of times the inner parser should match.
  #[inline(always)]
  pub const fn maximum(&self) -> Maximum {
    self.maximum
  }

  /// Returns the minimum number of times the inner parser should match.
  #[inline(always)]
  pub const fn minimum(&self) -> Minimum {
    self.minimum
  }

  define_many_delimited_methods!();

  /// Returns a mutable reference to the inner parser.
  #[inline(always)]
  pub const fn parser_mut(&mut self) -> &mut P {
    &mut self.parser
  }

  /// Returns a mutable `Bounded` parser with a mutable reference to the inner parser.
  #[inline(always)]
  pub const fn as_mut(&mut self) -> Bounded<&mut P> {
    Bounded {
      maximum: self.maximum,
      minimum: self.minimum,
      parser: &mut self.parser,
    }
  }

  #[inline(always)]
  pub(crate) const fn to_with(&self) -> With<Minimum, Maximum> {
    With::new(self.minimum(), self.maximum())
  }

  /// Maps the inner parser to a new parser using the given function.
  #[inline(always)]
  pub fn map_parser_mut<'a, F, NP>(&'a mut self, f: F) -> Bounded<NP>
  where
    F: FnOnce(&'a mut P) -> NP,
    NP: 'a,
  {
    Bounded {
      maximum: self.maximum,
      minimum: self.minimum,
      parser: f(&mut self.parser),
    }
  }
}
