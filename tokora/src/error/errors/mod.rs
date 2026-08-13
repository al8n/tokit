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
///
/// # Construction is an insertion door too
///
/// Removing the mutable views closed every door that *mutates* an already-wrapped container. It
/// did not close the ones that *build* one, and a truncating construction reaches the same lie
/// by a route that mutates nothing: the container arrives already full, already short, and the
/// flag is initialised to `false` (al8n/tokora#284). So the conversions that are handed **errors**
/// — [`FromIterator`] and [`From<E>`](From) — are bounded on
/// [`ErrorContainer`](super::ErrorContainer) and take the container's own accounting answer:
/// [`From<E>`](From) offers its one error to [`try_push`](Self::try_push), and `FromIterator`
/// hands the whole iterator to
/// [`ErrorContainer::from_errors`](super::ErrorContainer::from_errors), which is a loop of that
/// same [`try_push`](Self::try_push) unless the container overrides it, and which reports what
/// the container refused either way. `FromIterator` is the case that made it worth a bound: it
/// has no channel for reporting a value the container refused, so a bounded `C` collecting more
/// errors than it holds can only drop them, and `collect()` is read as lossless.
///
/// [`from_container`](Self::from_container) is the one construction door that is **not** an
/// insertion door, and the difference is who did the dropping. There the caller builds the
/// container and hands it over whole; the wrapper cannot know its history and no type could, so
/// the flag starts `false` and covers only what is offered afterwards. That obligation is
/// discharge-able rather than residual — a caller filling a bounded container *from errors* has
/// `collect()`, `From`, `push` and `try_push`, all of which account — and it is stated on that
/// method.
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
  /// error: no mutable view reaches the container's own insertion API, and the conversions that
  /// are handed errors ([`FromIterator`], [`From<E>`](From)) ask the container what it refused
  /// rather than assuming it refused nothing — through [`try_push`](Self::try_push) for a single
  /// error, and through
  /// [`ErrorContainer::from_errors`](super::ErrorContainer::from_errors) for an iterator of them,
  /// which is a loop of that same `try_push` unless the container overrides it. See the notes on
  /// the type.
  ///
  /// It is a fact about *this* collection. [`clone`](Clone::clone) carries it, because the clone
  /// is the same collection's history; consuming this one with
  /// [`into_iter`](IntoIterator::into_iter) and collecting the yielded errors into another does
  /// not, because that is a different collection and it kept everything it was offered. The
  /// dropped error is not among them to be re-offered.
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

/// Collects every error through [`push`](Errors::push), so a bounded `Container` that cannot
/// take them all latches [`overflowed`](Errors::overflowed) instead of dropping them in silence.
///
/// The bound is [`ErrorContainer`](super::ErrorContainer) and not `Container: FromIterator<E>`,
/// which is a swap rather than an addition: it is exactly the bound under which
/// `overflowed`/`push`/`pop` exist at all, so no `Errors` that can *report* an overflow loses its
/// `collect()`. What it costs is `collect()` into a `Container` that is `FromIterator<E>` and not
/// an `ErrorContainer` — an `Errors` with no insertion, removal or accounting surface, only the
/// shared views. Those build with [`from_container`](Errors::from_container).
///
/// It also *gains* the containers that have no `FromIterator` of their own, which includes both
/// of this crate's bounded ones: `Option<E>` and `GenericArrayDeque<E, N>`, and so
/// [`DefaultContainer`] in a no-alloc build.
///
/// # Why the funnel is the *default* rather than the mechanism
///
/// Offering the errors one at a time is what makes the count honest, and for a bounded container
/// it is also the only thing that can be done. For an unbounded one it is a tax with no accounting
/// to show for it, and the tax is not a constant factor: `Vec`'s and `VecDeque`'s own
/// `FromIterator` specialise on a `Vec`/`VecDeque` `IntoIter` and take the source's allocation
/// outright, where a per-item fill has to hold the source buffer alive beside an equally large
/// destination until the last error is copied across. Over a million `u64` that is a peak of twice
/// the source instead of once it.
///
/// **Peak is not the axis that matters.** Between roughly one and two times the source in
/// available memory, the delegated fill completes and a per-item one asks for a second buffer it
/// cannot get — and a failed allocation is not a diagnostic. It is `handle_alloc_error`, which
/// aborts the process. So the funnel does not make an unbounded `collect()` slower, it roughly
/// halves the largest error count that survives at a given ceiling. Measured at a 12 MiB ceiling:
/// 1 572 864 errors collected, against 786 432 for the per-item fill, and the difference is an
/// abort rather than a truncation.
///
/// So the choice of fill is [`ErrorContainer::from_errors`](super::ErrorContainer::from_errors), a
/// bulk-construction hook on the container that **defaults to that per-item funnel** and returns
/// the overflow state alongside the container. A container that leaves it alone accounts exactly
/// as before; `Vec` and `VecDeque` override it with their own `FromIterator` and report `false`,
/// which is sound because neither can refuse an error rather than because std specialises. If the
/// specialisation ever stops firing, an unbounded `collect()` costs what the funnel costs today —
/// the improvement rests on it, never the accounting.
///
/// **The hook trusts the container no further than [`try_push`](Errors::try_push) already does.**
/// An override that keeps only what fits and answers `false` makes
/// [`overflowed`](Errors::overflowed) report clean over dropped errors — which is precisely what a
/// `try_push` that answers `Ok(())` after dropping the error does, on a trait whose whole purpose
/// is to be implemented by callers.
/// The bulk door is the same door, one call wider.
impl<E, Container> FromIterator<E> for Errors<E, Container>
where
  Container: super::ErrorContainer<E>,
{
  #[inline]
  fn from_iter<I: IntoIterator<Item = E>>(iter: I) -> Self {
    let (container, overflowed_flag) = Container::from_errors(iter);
    Self {
      container,
      overflowed_flag,
      _phantom: core::marker::PhantomData,
    }
  }
}

/// Wraps a single error, latching [`overflowed`](Errors::overflowed) in the one case that cannot
/// hold it: a `C` whose capacity is zero.
///
/// The conversion stays infallible. A capacity *floor* is what would make it unconditionally
/// lossless, and there is nothing to express one with — [`ErrorContainer`](super::ErrorContainer)
/// reports capacity through `remaining_capacity(&self)`, a reading of an instance, and an
/// associated `const MIN_CAPACITY` would be a caller's declaration, which is the class of promise
/// this type already refuses to rest on. So the zero-capacity case is answered by the flag rather
/// than by a `Result`, and every other `C` behaves exactly as before.
impl<E, C> From<E> for Errors<E, C>
where
  C: super::ErrorContainer<E>,
{
  #[inline]
  fn from(error: E) -> Self {
    let mut this = Self::with_capacity(1);
    this.push(error);
    this
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

  /// Adopts an existing container as the collection's starting state.
  ///
  /// [`overflowed`](Self::overflowed) starts `false` and reports only what is offered to this
  /// wrapper afterwards. That is the only value it can start at: whether `container`'s own
  /// construction refused an error is a fact the container does not record and this type cannot
  /// read. **It is a logic error to build `container` by a route that dropped errors and then
  /// read `overflowed()` as covering them** — fill a bounded container *from errors* with
  /// [`FromIterator`], [`From<E>`](From), [`push`](Self::push) or [`try_push`](Self::try_push),
  /// each of which accounts for what did not fit.
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
