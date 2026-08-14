//! Generic container for invalid hexadecimal digit characters.
//!
//! This module provides a zero-copy, stack-allocated container for storing
//! invalid hex digit characters encountered during escape sequence parsing.
//!
//! # Design Philosophy
//!
//! Different escape sequence formats require different numbers of hex digits:
//! - Hex escapes (`\xXX`): 2 digits
//! - Fixed unicode escapes (`\uXXXX`): 4 digits
//!
//! This generic container can be specialized for each format while sharing
//! the same implementation. Internally, it uses [`GenericArrayDeque`] for efficient
//! stack-based storage.
//!
//! # Examples
//!
//! ## Const-Generic Version (Default)
//!
//! ```rust
//! # {
//! use tokora::error::InvalidHexDigits;
//!
//! // For hex escapes (\xXX) - max 2 digits
//! let mut hex_digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
//! assert_eq!(hex_digits.len(), 1);
//! # }
//! ```

use core::ops::AddAssign;

use generic_arraydeque::{ConstArrayLength, GenericArrayDeque, IntoArrayLength, typenum::Const};

use crate::utils::{PositionedChar, human_display::DisplayHuman};

/// A zero-copy container for storing invalid hex digit characters.
///
/// This structure uses const generics to specify the maximum number of invalid
/// characters it can hold. When parsing hex escape sequences fails, this container
/// holds the invalid characters encountered (up to `N`) with their positions,
/// enabling precise error reporting without heap allocation.
///
/// # Design
///
/// The container wraps [`GenericArrayDeque`] which provides stack-based storage optimized
/// for small sizes. It implements `Deref<Target = [PositionedChar<Char>]>` for
/// convenient access to the stored characters.
///
/// # Examples
///
/// ## For Hex Escapes (N=2)
///
/// ```
/// use tokora::error::InvalidHexDigits;
/// use tokora::utils::PositionedChar;
///
/// // Hex escapes need max 2 digits
/// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_positioned_char(PositionedChar::with_position('G', 12));
/// digits.push(PositionedChar::with_position('H', 13));
/// assert_eq!(digits.len(), 2);
/// ```
///
/// ## For Unicode Escapes (N=4)
///
/// ```
/// use tokora::error::InvalidHexDigits;
/// use tokora::utils::PositionedChar;
///
/// // Unicode escapes need max 4 digits
/// let mut digits: InvalidHexDigits<char, 4> = InvalidHexDigits::from_positioned_char(PositionedChar::with_position('G', 12));
/// digits.push(PositionedChar::with_position('H', 13));
/// digits.push(PositionedChar::with_position('I', 14));
/// digits.push(PositionedChar::with_position('J', 15));
/// assert_eq!(digits.len(), 4);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InvalidHexDigits<Char, const N: usize, O = usize>(
  GenericArrayDeque<PositionedChar<Char, O>, ConstArrayLength<N>>,
)
where
  Const<N>: IntoArrayLength;

impl<Char, const N: usize, O> core::fmt::Display for InvalidHexDigits<Char, N, O>
where
  Char: DisplayHuman,
  Const<N>: IntoArrayLength,
  O: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    let mut first = true;
    for ch in self.iter() {
      if !first {
        write!(f, ", ")?;
      }
      write!(
        f,
        "'{}' at position {}",
        ch.char_ref().display(),
        ch.position_ref()
      )?;
      first = false;
    }
    Ok(())
  }
}

impl<Char, const N: usize, O> From<PositionedChar<Char, O>> for InvalidHexDigits<Char, N, O>
where
  Const<N>: IntoArrayLength,
{
  #[inline]
  fn from(c: PositionedChar<Char, O>) -> Self {
    Self::from_positioned_char(c)
  }
}

impl<Char, const N: usize, O> From<[PositionedChar<Char, O>; 1]> for InvalidHexDigits<Char, N, O>
where
  Const<N>: IntoArrayLength,
{
  #[inline]
  fn from(c: [PositionedChar<Char, O>; 1]) -> Self {
    let [c] = c;
    Self::from_positioned_char(c)
  }
}

impl<Char, const N: usize, O> InvalidHexDigits<Char, N, O>
where
  Const<N>: IntoArrayLength,
{
  /// Creates a new `InvalidHexDigits` containing a single invalid digit.
  ///
  /// ## Panics
  ///
  /// - Panics if `N` is zero.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{error::InvalidHexDigits, utils::PositionedChar};
  ///
  /// let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_positioned_char(PositionedChar::with_position('Z', 12));
  /// ```
  #[inline]
  pub fn from_positioned_char(ch: PositionedChar<Char, O>) -> Self {
    assert!(N > 0, "InvalidHexDigits capacity must be > 0");

    let mut vec = GenericArrayDeque::new();
    vec.push_back(ch);
    Self(vec)
  }

  /// Creates a new `InvalidHexDigits` containing a single invalid digit.
  ///
  /// ## Panics
  ///
  /// - Panics if `N` is zero.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::InvalidHexDigits;
  ///
  /// let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(12, 'Z');
  /// ```
  #[inline]
  pub fn from_char(pos: O, ch: Char) -> Self {
    Self::from_positioned_char(PositionedChar::with_position(ch, pos))
  }

  /// Creates a new `InvalidHexDigits` from an array of characters.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{error::InvalidHexDigits, utils::PositionedChar};
  ///
  /// let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_array([
  ///   PositionedChar::with_position('G', 10),
  ///   PositionedChar::with_position('H', 11),
  /// ]);
  /// assert_eq!(digits.len(), 2);
  /// ```
  pub fn from_array(chars: [PositionedChar<Char, O>; N]) -> Self {
    Self(GenericArrayDeque::from_array(chars))
  }

  /// Creates a new `InvalidHexDigits` from an iterator.
  ///
  /// Returns `None` if the iterator yields more than `N` items.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::InvalidHexDigits;
  /// use tokora::utils::PositionedChar;
  ///
  /// let chars = vec![
  ///     PositionedChar::with_position('G', 10),
  ///     PositionedChar::with_position('H', 11),
  /// ];
  ///
  /// let digits: InvalidHexDigits<char, 2> =
  ///     InvalidHexDigits::try_from_iter(chars).unwrap();
  /// assert_eq!(digits.len(), 2);
  /// ```
  #[inline]
  pub fn try_from_iter<I>(iter: I) -> Option<Self>
  where
    I: IntoIterator<Item = PositionedChar<Char, O>>,
  {
    GenericArrayDeque::try_from_iter(iter).map(Self).ok()
  }

  /// Pushes an invalid hex digit to the container.
  ///
  /// Returns `true` if the digit was added, `false` if the container is full.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::InvalidHexDigits;
  /// use tokora::utils::PositionedChar;
  ///
  /// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_positioned_char(PositionedChar::with_position('G', 10));
  /// assert!(digits.push(PositionedChar::with_position('H', 11)));
  /// assert!(!digits.push(PositionedChar::with_position('I', 12))); // Full!
  /// ```
  #[inline]
  pub fn push(&mut self, ch: PositionedChar<Char, O>) -> bool {
    self.0.push_back(ch).is_none()
  }

  /// Pushes an invalid hex digit to the container.
  ///
  /// Returns `true` if the digit was added, `false` if the container is full.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::InvalidHexDigits;
  /// use tokora::utils::PositionedChar;
  ///
  /// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
  /// assert!(digits.push_char(11, 'H'));
  /// assert!(!digits.push_char(12, 'I')); // Full!
  /// ```
  #[inline]
  pub fn push_char(&mut self, pos: O, ch: Char) -> bool {
    self.push(PositionedChar::with_position(ch, pos))
  }

  /// Returns the number of invalid hex digits stored.
  ///
  /// The length will be in the range `0..=N`.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::InvalidHexDigits;
  /// use tokora::utils::PositionedChar;
  ///
  /// let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from(
  ///     PositionedChar::with_position('Z', 5)
  /// );
  /// assert_eq!(digits.len(), 1);
  /// ```
  #[inline]
  #[allow(clippy::len_without_is_empty)]
  pub const fn len(&self) -> usize {
    self.0.len()
  }

  /// Returns `true` if the container is at maximum capacity.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::InvalidHexDigits;
  /// use tokora::utils::PositionedChar;
  ///
  /// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
  /// assert!(!digits.is_full());
  /// digits.push(PositionedChar::with_position('H', 11));
  /// assert!(digits.is_full());
  /// ```
  #[inline]
  pub const fn is_full(&self) -> bool {
    self.0.is_full()
  }

  /// Bumps the position of all stored characters by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining errors from different parsing contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::InvalidHexDigits;
  /// use tokora::utils::PositionedChar;
  ///
  /// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from(
  ///     PositionedChar::with_position('G', 10)
  /// );
  /// digits.bump(&5);
  /// assert_eq!(digits[0].position(), 15);
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O>,
  {
    let mut idx = 0;
    let slice = self.0.as_mut_slices().0;
    while idx < slice.len() {
      slice[idx].bump_position(n);
      idx += 1;
    }
    self
  }
}

impl<Char, const N: usize, O> AsRef<[PositionedChar<Char, O>]> for InvalidHexDigits<Char, N, O>
where
  Const<N>: IntoArrayLength,
{
  #[inline]
  fn as_ref(&self) -> &[PositionedChar<Char, O>] {
    self
  }
}

impl<Char, const N: usize, O> AsMut<[PositionedChar<Char, O>]> for InvalidHexDigits<Char, N, O>
where
  Const<N>: IntoArrayLength,
{
  #[inline]
  fn as_mut(&mut self) -> &mut [PositionedChar<Char, O>] {
    self
  }
}

impl<Char, const N: usize, O> core::ops::Deref for InvalidHexDigits<Char, N, O>
where
  Const<N>: IntoArrayLength,
{
  type Target = [PositionedChar<Char, O>];

  #[inline]
  fn deref(&self) -> &Self::Target {
    self.0.as_slices().0
  }
}

impl<Char, const N: usize, O> core::ops::DerefMut for InvalidHexDigits<Char, N, O>
where
  Const<N>: IntoArrayLength,
{
  #[inline]
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.0.as_mut_slices().0
  }
}
