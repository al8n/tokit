use super::*;

/// A parser that allows leading separators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AllowLeading<P> {
  pub(in crate::parser) parser: P,
}

impl<P> AllowLeading<P> {
  /// Creates a new `AllowLeading` parser that allows leading separators.
  #[inline(always)]
  pub const fn new(parser: P) -> Self {
    Self { parser }
  }

  /// Sets the parser to allow trailing separators.
  #[inline(always)]
  pub fn allow_trailing(self) -> AllowLeading<AllowTrailing<P>> {
    AllowLeading {
      parser: AllowTrailing {
        parser: self.parser,
      },
    }
  }

  /// Sets the parser to allow trailing separators.
  #[inline(always)]
  pub fn require_trailing(self) -> AllowLeading<RequireTrailing<P>> {
    AllowLeading {
      parser: RequireTrailing {
        parser: self.parser,
      },
    }
  }

  /// Sets the maximum number of elements to parse.
  #[inline(always)]
  pub fn at_most(self, maximum: usize) -> AllowLeading<AtMost<P>> {
    AllowLeading {
      parser: AtMost::new(self.parser, maximum),
    }
  }

  /// Sets the minimum number of elements to parse.
  #[inline(always)]
  pub fn at_least(self, minimum: usize) -> AllowLeading<AtLeast<P>> {
    AllowLeading {
      parser: AtLeast::new(self.parser, minimum),
    }
  }

  /// Sets both the minimum and maximum number of elements to parse.
  #[inline(always)]
  pub fn bounded(self, minimum: usize, maximum: usize) -> AllowLeading<Bounded<P>> {
    AllowLeading {
      parser: Bounded::new(self.parser, maximum, minimum),
    }
  }

  /// Returns a mutable reference to the inner parser.
  #[inline(always)]
  pub const fn parser_mut(&mut self) -> &mut P {
    &mut self.parser
  }

  define_many_delimited_methods!();

  /// Returns a mutable reference to the `AllowLeading` parser wrapping the inner parser.
  #[inline(always)]
  pub const fn as_mut(&mut self) -> AllowLeading<&mut P> {
    AllowLeading {
      parser: &mut self.parser,
    }
  }

  /// Maps the inner parser to a new parser using the given function.
  #[inline(always)]
  pub fn map_parser_mut<'a, F, NP>(&'a mut self, f: F) -> AllowLeading<NP>
  where
    F: FnOnce(&'a mut P) -> NP,
    NP: 'a,
  {
    AllowLeading {
      parser: f(&mut self.parser),
    }
  }
}
