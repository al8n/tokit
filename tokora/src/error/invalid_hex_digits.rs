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

use generic_arraydeque::{GenericArrayDeque, IntoArrayLength, typenum::Const};

use crate::utils::{PositionedChar, human_display::DisplayHuman};

pub use store::InvalidHexDigits;

/// The backing ring, and the only code in the crate that can reach it.
///
/// [`InvalidHexDigits`] is declared here rather than in the parent module so that its field is
/// *unreachable* from the parent: a private field is visible in the module that declares the
/// struct and in that module's descendants, and the parent module is neither. Every operation
/// the parent performs on the ring therefore goes through one of the doors below, and the door
/// that can move the head is the door that restores contiguity.
///
/// # The property this protects
///
/// The parent hands out `&[PositionedChar<Char, O>]` through [`Deref`](core::ops::Deref), whose
/// receiver is `&self` by trait definition. A shared borrow cannot normalize a ring, and one
/// `&[T]` cannot describe two physical segments, so a shared reader can only return the ring's
/// first segment — which is the whole set exactly while the head is at zero. Handing out that
/// first segment as if it were the whole set is the shape tokora#245 fixed in `IncompleteSyntax`
/// and tokora#280 found latent here.
///
/// Today the head is at zero because the only mutation is an append. But that is a property of
/// a method list, and a method list is not a type: a `pop_front`, `remove`, `rotate_left` or any
/// other head-moving operation added later would silently turn the shared reader into a
/// truncating one, three read sites away from the method that caused it, and two of those sites
/// do not name `as_slices` at all.
///
/// So the head-moving operations are not forbidden here, they are *funnelled*: such a method
/// cannot be written without `InvalidHexDigits::with_ring`, and `with_ring` re-establishes
/// contiguity on every exit from the closure, the unwinding one included. A contributor who
/// reaches for `self.0.pop_front()` in the parent module gets a private-field error rather than
/// a latent reintroduction of #245.
mod store {
  use generic_arraydeque::{
    ArrayLength, ConstArrayLength, GenericArrayDeque, IntoArrayLength, typenum::Const,
  };

  use crate::utils::PositionedChar;

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
  /// convenient access to the stored characters, and that accessor reports **every** stored
  /// character rather than a physical prefix of them — see the module this type is declared in
  /// for how the ring is kept in one segment.
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

  /// Restores contiguity when an exclusive borrow of the ring ends, on **every** exit path.
  ///
  /// A normalization that only runs where the closure returns normally is not a postcondition:
  /// an unwind out of a caller-supplied closure — a panicking `retain` predicate, say — would
  /// leave the ring wrapped and every later shared read silently short. `Drop` is what makes
  /// the two paths the same path.
  struct Normalized<'ring, T, N: ArrayLength>(&'ring mut GenericArrayDeque<T, N>);

  impl<T, N: ArrayLength> Drop for Normalized<'_, T, N> {
    #[inline]
    fn drop(&mut self) {
      self.0.make_contiguous();
    }
  }

  impl<Char, const N: usize, O> InvalidHexDigits<Char, N, O>
  where
    Const<N>: IntoArrayLength,
  {
    /// Wraps a ring, contiguous, whatever the caller handed over.
    #[inline]
    pub(super) fn from_ring(
      ring: GenericArrayDeque<PositionedChar<Char, O>, ConstArrayLength<N>>,
    ) -> Self {
      let mut this = Self(ring);
      this.0.make_contiguous();
      this
    }

    /// The only exclusive door to the ring, and therefore the only place a future head-moving
    /// operation can be written.
    ///
    /// The closure's argument is higher-ranked, so no borrow of the ring — and in particular no
    /// borrow of one of its two physical segments — outlives the call.
    #[inline]
    pub(super) fn with_ring<R>(
      &mut self,
      f: impl FnOnce(&mut GenericArrayDeque<PositionedChar<Char, O>, ConstArrayLength<N>>) -> R,
    ) -> R {
      let guard = Normalized(&mut self.0);
      f(&mut *guard.0)
    }

    /// Every stored digit, by shared reference.
    ///
    /// This is the read that cannot normalize anything, so it is the read that depends on the
    /// funnel above having done it. The assertion is where that dependency is checked: if an
    /// insertion or removal path is ever added that leaves the ring wrapped, this accessor is
    /// the one that would silently report a prefix, so this is where it says so.
    #[inline]
    pub(super) fn digits(&self) -> &[PositionedChar<Char, O>] {
      let (contiguous, wrapped) = self.0.as_slices();
      debug_assert!(
        wrapped.is_empty(),
        "InvalidHexDigits: the digit ring is wrapped, so this accessor is about to report \
         {} of {} digits — a mutation path bypassed `with_ring`",
        contiguous.len(),
        self.0.len(),
      );
      contiguous
    }

    /// Every stored digit, by exclusive reference.
    ///
    /// Unlike `digits` this one owes nothing to the funnel: `make_contiguous`
    /// *is* the normalization, so the slice it returns is the whole ring by construction
    /// whatever state the ring was in.
    #[inline]
    pub(super) fn digits_mut(&mut self) -> &mut [PositionedChar<Char, O>] {
      self.0.make_contiguous()
    }

    /// The number of stored digits. A shared read that does not depend on contiguity.
    #[inline]
    pub(super) const fn stored(&self) -> usize {
      self.0.len()
    }

    /// Whether the ring is at capacity. A shared read that does not depend on contiguity.
    #[inline]
    pub(super) const fn saturated(&self) -> bool {
      self.0.is_full()
    }
  }
}

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
    Self::from_ring(vec)
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
    Self::from_ring(GenericArrayDeque::from_array(chars))
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
    GenericArrayDeque::try_from_iter(iter)
      .map(Self::from_ring)
      .ok()
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
    self.with_ring(|ring| ring.push_back(ch)).is_none()
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
    self.stored()
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
    self.saturated()
  }

  /// Bumps the position of all stored characters by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining errors from different parsing contexts.
  ///
  /// Reads the whole ring rather than its first physical segment. The receiver is exclusive,
  /// so this site normalizes instead of assuming — which is what the two reads it replaced
  /// could not do, and why tokora#280 found them.
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
    for ch in self.digits_mut() {
      ch.bump_position(n);
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
    self.digits()
  }
}

impl<Char, const N: usize, O> core::ops::DerefMut for InvalidHexDigits<Char, N, O>
where
  Const<N>: IntoArrayLength,
{
  #[inline]
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.digits_mut()
  }
}
