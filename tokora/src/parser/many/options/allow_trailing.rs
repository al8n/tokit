use super::*;

/// A parser that allows trailing separators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AllowTrailing<P> {
  pub(in crate::parser) parser: P,
}

impl<P> AllowTrailing<P> {
  /// Creates a new `AllowTrailing` parser that allows trailing separators.
  #[inline(always)]
  pub const fn new(parser: P) -> Self {
    Self { parser }
  }

  /// Sets the parser to allow trailing separators.
  #[inline(always)]
  pub const fn allow_leading(self) -> AllowLeading<AllowTrailing<P>> {
    AllowLeading::new(self)
  }

  /// Sets the parser to require trailing separators.
  #[inline(always)]
  pub fn require_leading(self) -> RequireLeading<AllowTrailing<P>> {
    RequireLeading::new(self)
  }

  /// Sets the maximum number of elements to parse.
  #[inline(always)]
  pub fn at_most(self, maximum: usize) -> AllowTrailing<AtMost<P>> {
    AllowTrailing::new(AtMost::new(self.parser, maximum))
  }

  /// Sets the minimum number of elements to parse.
  #[inline(always)]
  pub fn at_least(self, minimum: usize) -> AllowTrailing<AtLeast<P>> {
    AllowTrailing::new(AtLeast::new(self.parser, minimum))
  }

  /// Sets both the minimum and maximum number of elements to parse.
  #[inline(always)]
  pub fn bounded(self, minimum: usize, maximum: usize) -> AllowTrailing<Bounded<P>> {
    AllowTrailing::new(Bounded::new(self.parser, maximum, minimum))
  }

  define_many_delimited_methods!();

  /// Returns a mutable reference to the inner parser.
  #[inline(always)]
  pub const fn parser_mut(&mut self) -> &mut P {
    &mut self.parser
  }

  /// Returns a mutable reference to the `AllowTrailing` parser wrapping the inner parser.
  #[inline(always)]
  pub const fn as_mut(&mut self) -> AllowTrailing<&mut P> {
    AllowTrailing {
      parser: &mut self.parser,
    }
  }

  /// Maps the inner parser to a new parser using the given function.
  #[inline(always)]
  pub fn map_parser_mut<'a, F, NP>(&'a mut self, f: F) -> AllowTrailing<NP>
  where
    F: FnOnce(&'a mut P) -> NP,
    NP: 'a,
  {
    AllowTrailing {
      parser: f(&mut self.parser),
    }
  }
}
