//! Error collection container that adapts to allocation environments.
//!
//! This module provides the `Errors` type for collecting multiple errors during parsing
//! or validation. The container automatically adapts based on available features:
//!
//! - **no_std (no alloc)**: Uses `ConstGenericArrayDeque<E, 2>` with fixed capacity of 2 errors
//! - **alloc/std**: Uses `VecDeque<E>` for unlimited error collection
//!
//! # Examples
//!
//! ## Basic Usage
//!
//! ```rust
//! use tokora::error::Errors;
//!
//! let mut errors = Errors::new();
//! errors.push("First error");
//! errors.push("Second error");
//!
//! assert_eq!(errors.len(), 2);
//! assert!(!errors.is_empty());
//! ```
//!
//! ## Iteration
//!
//! ```rust
//! use tokora::error::Errors;
//!
//! let mut errors = Errors::new();
//! errors.push(1);
//! errors.push(2);
//!
//! let sum: i32 = errors.iter().sum();
//! assert_eq!(sum, 3);
//! ```

use core::fmt::{Debug, Display};

#[cfg(not(any(feature = "alloc", feature = "std")))]
use generic_arraydeque::ConstGenericArrayDeque;

#[cfg(any(feature = "alloc", feature = "std"))]
use std::collections::VecDeque;

/// Default error container for no-alloc environments.
///
/// Uses a stack-allocated `ConstGenericArrayDeque` with capacity for 2 errors.
/// When the capacity is exceeded, additional errors are dropped and
/// [`Errors::overflowed`](Errors::overflowed) becomes `true`.
#[cfg(not(any(feature = "alloc", feature = "std")))]
pub type DefaultContainer<E> = ConstGenericArrayDeque<E, 2>;

/// Default error container for alloc/std environments.
///
/// Uses a heap-allocated `VecDeque` for unlimited error collection.
#[cfg(any(feature = "alloc", feature = "std"))]
pub type DefaultContainer<E> = VecDeque<E>;

/// A collection of errors that adapts to the allocation environment.
///
/// This type is generic over both the error type `E` and the container `C`.
/// By default:
/// - In no-alloc environments: Uses `ConstGenericArrayDeque<E, 2>` (capacity of 2)
/// - In alloc/std environments: Uses `VecDeque<E>` (unlimited capacity)
///
/// # Type Parameters
///
/// - `E`: The error type to store
/// - `C`: The container type (defaults to environment-appropriate container)
///
/// # Examples
///
/// ## Using Default Container
///
/// ```rust
/// use tokora::error::Errors;
///
/// let mut errors = Errors::new();
/// errors.push("Error 1");
/// errors.push("Error 2");
///
/// assert_eq!(errors.len(), 2);
/// ```
///
/// ## Type Inference
///
/// ```rust
/// use tokora::error::Errors;
///
/// // Type inference works seamlessly
/// let mut errors = Errors::new();
/// errors.push("Error 1");
/// errors.push("Error 2");
///
/// let first: Option<&&str> = errors.front();
/// assert_eq!(first, Some(&"Error 1"));
/// ```
/// # Why there is no mutable door onto the container
///
/// `overflowed_flag` is wrapper metadata about something the container does not record: that an
/// error was *offered* and did not fit. Only an insertion can create that fact, so only the
/// wrapper's own insertion gateway ([`push`](Self::push) / [`try_push`](Self::try_push)) can
/// maintain it — and a `DerefMut`/`AsMut<C>` onto `C` handed callers the container's own
/// insertion API, through which a bounded container rejects a value and
/// [`overflowed`](Self::overflowed) never learns of it (al8n/tokora#247). Both doors are gone.
/// Documenting the obligation was the alternative, and it is the kind of promise the compiler
/// does not keep: the flag cannot be *derived* from the container either, because a full
/// container and a full container that has refused ten errors are the same container.
///
/// What survives is everything that cannot invent that fact: the shared views
/// ([`Deref`](core::ops::Deref), [`AsRef`](core::convert::AsRef)), in-place element mutation
/// ([`iter_mut`](Self::iter_mut), and `AsMut<[E]>` where the container is contiguous), and
/// removal ([`pop`](Self::pop), [`clear`](Self::clear)) — removal cannot un-drop an error, so
/// the flag is a historical fact and stays set.
///
/// `C` is the caller's own type and Rust has no bound that excludes interior mutability, so a
/// container that mutates itself through `&self` will do so through the shared views. **It is a
/// logic error for the container to be modified, after it is wrapped, in a way that changes what
/// this type reports** — the same obligation
/// [`HashSet`](https://doc.rust-lang.org/std/collections/struct.HashSet.html) states for a key,
/// and, like it, the behavior that follows is unspecified rather than undefined and is
/// deliberately not enumerated.
#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Deref, derive_more::AsRef)]
pub struct Errors<E, C = DefaultContainer<E>> {
  #[deref]
  #[as_ref]
  container: C,
  overflowed_flag: bool,
  _phantom: core::marker::PhantomData<E>,
}

// Implementation for no-alloc environments (ConstGenericArrayDeque)
#[cfg(not(any(feature = "alloc", feature = "std")))]
impl<E> Errors<E> {
  /// Creates a new empty error collection.
  ///
  /// In no-alloc environments, this creates a `ConstGenericArrayDeque` with capacity 2.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::error::Errors;
  ///
  /// let errors: Errors<String> = Errors::new();
  /// assert!(errors.is_empty());
  /// ```
  #[inline]
  pub const fn new() -> Self {
    Self::new_in(DefaultContainer::new())
  }
}

// Implementation for alloc/std environments (VecDeque)
#[cfg(any(feature = "alloc", feature = "std"))]
impl<E> Errors<E> {
  /// Creates a new empty error collection.
  ///
  /// In alloc/std environments, this creates an empty `VecDeque`.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::error::Errors;
  ///
  /// let errors: Errors<String> = Errors::new();
  /// assert!(errors.is_empty());
  /// ```
  #[inline]
  pub const fn new() -> Self {
    Self::new_in(VecDeque::new())
  }

  /// Returns the number of errors the collection can hold without reallocating.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::error::Errors;
  ///
  /// let errors: Errors<String> = Errors::with_capacity(10);
  /// assert_eq!(errors.capacity(), 10);
  /// ```
  #[inline]
  pub fn capacity(&self) -> usize {
    self.container.capacity()
  }

  /// Reserves capacity for at least `additional` more errors.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::error::Errors;
  ///
  /// let mut errors: Errors<String> = Errors::new();
  /// errors.reserve(10);
  /// assert!(errors.capacity() >= 10);
  /// ```
  #[inline]
  pub fn reserve(&mut self, additional: usize) {
    self.container.reserve(additional);
  }
}

impl<E, Container> Errors<E, Container>
where
  Container: super::ErrorContainer<E>,
{
  /// Pushes an error into the collection, marking `overflowed` if it doesn't fit.
  #[inline(always)]
  pub fn push(&mut self, error: E) {
    let _ = self.try_push(error);
  }

  /// Attempts to push an error, returning it back if capacity is exhausted.
  #[inline(always)]
  pub fn try_push(&mut self, error: E) -> Result<(), E> {
    match super::ErrorContainer::try_push(&mut self.container, error) {
      Ok(()) => Ok(()),
      Err(err) => {
        self.overflowed_flag = true;
        Err(err)
      }
    }
  }

  /// Returns `true` if any error has **ever** been dropped because of limited capacity.
  ///
  /// A historical fact, not a reading of the container: it latches on the first rejected error
  /// and no removal clears it, because removing an error that *is* held does not un-drop one
  /// that never was. So a caller reading `false` may conclude that every error offered to
  /// [`push`](Self::push) / [`try_push`](Self::try_push) is accounted for, whatever the
  /// container has since done with them.
  ///
  /// That conclusion is only sound because those two are the *only* doors that can offer an
  /// error; see the note on the type about why there is no mutable door onto the container.
  #[inline(always)]
  pub const fn overflowed(&self) -> bool {
    self.overflowed_flag
  }

  /// Removes and returns the oldest collected error.
  ///
  /// The replacement for reaching the container's own removal API through `DerefMut`. Removal
  /// cannot invalidate [`overflowed`](Self::overflowed) — it is a fact about errors that never
  /// entered — so this door needs no accounting of its own.
  #[inline(always)]
  pub fn pop(&mut self) -> Option<E> {
    super::ErrorContainer::pop(&mut self.container)
  }

  /// Discards every collected error, leaving [`overflowed`](Self::overflowed) as it was.
  #[inline(always)]
  pub fn clear(&mut self) {
    super::ErrorContainer::clear(&mut self.container);
  }

  /// Reports the remaining capacity when the backing container is bounded.
  #[inline(always)]
  pub fn remaining_capacity(&self) -> Option<usize> {
    super::ErrorContainer::remaining_capacity(&self.container)
  }

  /// Returns `true` when a bounded container cannot accept more errors.
  #[inline(always)]
  pub fn is_full(&self) -> bool {
    matches!(self.remaining_capacity(), Some(0))
  }

  /// Creates a new empty error collection with the specified capacity.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::error::Errors;
  ///
  /// let errors: Errors<String> = Errors::with_capacity(5);
  /// assert_eq!(errors.len(), 0);
  /// ```
  #[inline(always)]
  pub fn with_capacity(capacity: usize) -> Self {
    Self::new_in(Container::with_capacity(capacity))
  }
}

impl<E, Container> Errors<E, Container> {
  #[inline]
  const fn new_in(container: Container) -> Self {
    Self {
      container,
      overflowed_flag: false,
      _phantom: core::marker::PhantomData,
    }
  }
}

// Default trait implementations
impl<E, Container> Default for Errors<E, Container>
where
  Container: Default,
{
  #[inline]
  fn default() -> Self {
    Self::new_in(Container::default())
  }
}

// AsRef and AsMut implementations
impl<E, C> AsRef<[E]> for Errors<E, C>
where
  C: AsRef<[E]>,
{
  #[inline]
  fn as_ref(&self) -> &[E] {
    self.container.as_ref()
  }
}

impl<E, C> AsMut<[E]> for Errors<E, C>
where
  C: AsMut<[E]>,
{
  #[inline]
  fn as_mut(&mut self) -> &mut [E] {
    self.container.as_mut()
  }
}

// Display implementation for better error reporting
impl<E, C> Display for Errors<E, C>
where
  E: Display,
  C: AsRef<[E]>,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    let errors = self.container.as_ref();

    if errors.is_empty() {
      return Ok(());
    }

    if errors.len() == 1 {
      write!(f, "{}", errors[0])
    } else {
      writeln!(f, "{} errors:", errors.len())?;
      for (i, error) in errors.iter().enumerate() {
        write!(f, "  {}. {}", i + 1, error)?;
        if i < errors.len() - 1 {
          writeln!(f)?;
        }
      }
      Ok(())
    }
  }
}

impl<'a, E, Container> IntoIterator for &'a Errors<E, Container>
where
  &'a Container: IntoIterator<Item = &'a E>,
{
  type Item = &'a E;
  type IntoIter = <&'a Container as IntoIterator>::IntoIter;

  #[inline]
  fn into_iter(self) -> Self::IntoIter {
    (&self.container).into_iter()
  }
}

impl<E, Container> IntoIterator for Errors<E, Container>
where
  Container: IntoIterator<Item = E>,
{
  type Item = E;
  type IntoIter = Container::IntoIter;

  #[inline]
  fn into_iter(self) -> Self::IntoIter {
    self.container.into_iter()
  }
}

impl<E, Container> FromIterator<E> for Errors<E, Container>
where
  Container: FromIterator<E>,
{
  #[inline]
  fn from_iter<I: IntoIterator<Item = E>>(iter: I) -> Self {
    Self::from_container(Container::from_iter(iter))
  }
}

impl<E, C> From<E> for Errors<E, C>
where
  C: FromIterator<E>,
{
  #[inline]
  fn from(error: E) -> Self {
    Self::from_iter(core::iter::once(error))
  }
}

impl<E, C> Errors<E, C> {
  /// Mutable access to the errors already collected, one at a time.
  ///
  /// Element mutation, never structural: the iterator yields `&mut E` and cannot add or remove
  /// an element, so it cannot create the dropped-error fact
  /// [`overflowed`](Self::overflowed) reports. It is the replacement for reaching the
  /// container's own `iter_mut` through `DerefMut`, and it is bounded on the *method* rather
  /// than on [`ErrorContainer`](super::ErrorContainer) so that adding it breaks no existing
  /// container implementation — every standard container satisfies it.
  #[inline]
  pub fn iter_mut<'a>(&'a mut self) -> <&'a mut C as IntoIterator>::IntoIter
  where
    &'a mut C: IntoIterator<Item = &'a mut E>,
  {
    (&mut self.container).into_iter()
  }

  /// Creates an `Errors` instance from an existing container.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// # #[cfg(any(feature = "alloc", feature = "std"))] {
  /// use tokora::error::{Errors, DefaultContainer};
  ///
  /// let errors = Errors::<&str, DefaultContainer<_>>::from_container(["Error 1", "Error 2"].into_iter().collect());
  /// assert_eq!(errors.len(), 2);
  /// # }
  /// ```
  #[inline(always)]
  pub const fn from_container(container: C) -> Self {
    Self::new_in(container)
  }
}

#[cfg(test)]
mod tests;
