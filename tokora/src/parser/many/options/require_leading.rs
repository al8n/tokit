use super::*;

/// A parser that requires leading separators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequireLeading<P> {
  pub(in crate::parser) parser: P,
}

impl<P> RequireLeading<P> {
  /// Creates a new `RequireLeading` parser that requires leading separators.
  #[inline(always)]
  pub const fn new(parser: P) -> Self {
    Self { parser }
  }

  /// Sets the parser to require trailing separator.
  #[inline(always)]
  pub fn require_trailing(self) -> RequireLeading<RequireTrailing<P>> {
    RequireLeading {
      parser: RequireTrailing {
        parser: self.parser,
      },
    }
  }

  /// Sets the parser to allow trailing separator.
  #[inline(always)]
  pub fn allow_trailing(self) -> RequireLeading<AllowTrailing<P>> {
    RequireLeading {
      parser: AllowTrailing {
        parser: self.parser,
      },
    }
  }

  /// Sets the maximum number of elements to parse.
  #[inline(always)]
  pub fn at_most(self, maximum: usize) -> RequireLeading<AtMost<P>> {
    RequireLeading {
      parser: AtMost::new(self.parser, maximum),
    }
  }

  /// Sets the minimum number of elements to parse.
  #[inline(always)]
  pub fn at_least(self, minimum: usize) -> RequireLeading<AtLeast<P>> {
    RequireLeading {
      parser: AtLeast::new(self.parser, minimum),
    }
  }

  /// Sets both the minimum and maximum number of elements to parse.
  #[inline(always)]
  pub fn bounded(self, minimum: usize, maximum: usize) -> RequireLeading<Bounded<P>> {
    RequireLeading {
      parser: Bounded::new(self.parser, maximum, minimum),
    }
  }

  /// Returns a mutable reference to the inner parser.
  #[inline(always)]
  pub const fn parser_mut(&mut self) -> &mut P {
    &mut self.parser
  }

  /// Returns a mutable reference to the `RequireLeading` parser wrapping the inner parser.
  #[inline(always)]
  pub const fn as_mut(&mut self) -> RequireLeading<&mut P> {
    RequireLeading {
      parser: &mut self.parser,
    }
  }

  define_many_delimited_methods!();

  /// Maps the inner parser to a new parser using the given function.
  #[inline(always)]
  pub fn map_parser_mut<'a, F, NP>(&'a mut self, f: F) -> RequireLeading<NP>
  where
    F: FnOnce(&'a mut P) -> NP,
    NP: 'a,
  {
    RequireLeading {
      parser: f(&mut self.parser),
    }
  }
}
