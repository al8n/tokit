use core::ops::RangeBounds;

use crate::Slice;

#[cfg(feature = "bytes_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bytes_1")))]
mod bytes_1;

#[cfg(feature = "bstr_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bstr_1")))]
mod bstr_1;

#[cfg(feature = "hipstr_0_8")]
#[cfg_attr(docsrs, doc(cfg(feature = "hipstr_0_8")))]
mod hipstr_0_8;

#[cfg(feature = "smol_bytes_0_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "smol_bytes_0_1")))]
mod smol_bytes_0_1;

/// The **offset origin and byte extent** of a borrowed source: where every event's offsets are
/// measured from, and how far the buffer they are measured into runs.
///
/// The two halves answer different questions and carry different relations, which is why they
/// are separate accessors rather than one equality:
///
/// - [`addr`](Self::addr) is the origin, and it is an **equality**. Two sides agree iff they
///   measure from the same place: `&buf[..5]` and `&buf[..12]` share an origin, because an
///   offset means the same thing in both; `&buf[1..]` does not, because every offset there
///   means something else.
/// - [`extent`](Self::extent) is the length, and it is an **ordering** — see
///   [`covers`](Self::covers).
///
/// # Why extent is carried, and why it is not an equality
///
/// An origin-only identity accepts a sink bound to `&buf[..2]` while the parse reads
/// `&buf[..3]`, and that pairing is not caught anywhere downstream. It needs no out-of-bounds
/// span to do damage: a parser can **peek** byte 2 — committing nothing — let it choose the
/// tree's shape, and commit only spans inside `0..2`. `finish` then materializes over the
/// sink's shorter text with no `SpanOutOfBounds` and no `UncoveredGap` to report, and the
/// round-trip law still holds, because `tree.text()` really is the sink's buffer. What escapes
/// is *structure shaped by bytes absent from the materialized source* — the wrong-source class,
/// surviving for same-allocation prefixes.
///
/// The reverse is not a defect at all. A sink bound to a **longer** extent than the parse reads
/// is the fixed-arena streaming direction: every span the parse produces lies inside the parse
/// extent, which is a subset of the sink's, so every slice is correct. The trailing bytes are
/// simply uncovered, and `finish` already owns that question — it reports
/// `FinishError::UncoveredGap`, or `finish_partial` tiles it, at the caller's choice.
///
/// So the two directions have opposite answers, and one representation must not stand for both.
/// A shorter-bound sink is refused; a longer-bound one is a capability.
///
/// # What an inequality does and does not prove
///
/// Equality proves *same value*: two live objects cannot share an address. **Inequality proves
/// only a different referent**, and whether that implies a different source depends on whether
/// `Self` is `?Sized`:
///
/// - an **unsized** backing (`str`, `[u8]`, `BStr`) *is* the data, so a different address is a
///   different source — proof;
/// - a **sized** backing addresses a *variable*, not the bytes. That covers the owned handles
///   (`bytes::Bytes`, `HipStr`, the `smol_bytes` family) **and the reference forms** — for
///   `L::Source = &str` an ordinary `let r2 = r1;` puts the copy at a different address while
///   both name the same buffer. A different address proves nothing.
///
/// [`Source::REFERENT_IS_BYTES`] is how a backing declares which case it is in, and it defaults
/// to the conservative answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceIdentity {
  origin: usize,
  extent: usize,
}

impl SourceIdentity {
  /// The offset origin and byte extent of `src`.
  ///
  /// `from_ref` is `?Sized`, and the `cast::<u8>()` drops the metadata, which is what makes
  /// `<*const u8>::addr` — a `T: Sized` method — reachable for an unsized `T`. `str`, `[u8]`
  /// and `BStr` views of one buffer all project to the same origin.
  ///
  /// The metadata the cast drops is exactly the extent, and `size_of_val` reads it back off the
  /// original reference: for `str`, `[u8]` and `BStr` — `#[repr(transparent)]` over `[u8]` —
  /// that is the byte length. It needs no bound on `T`, which is why the extent is available
  /// here rather than only at a `Source`-bounded call site.
  ///
  /// For a **sized** backing the extent is the size of the handle, not of the bytes, and means
  /// nothing. That is not a hazard because it is not reachable: the sole consumer runs only
  /// where [`Source::REFERENT_IS_BYTES`] is `true`, and exactly the unsized backings answer
  /// `true`.
  #[inline(always)]
  #[must_use]
  pub fn of<T: ?Sized>(src: &T) -> Self {
    Self {
      origin: core::ptr::from_ref(src).cast::<u8>().addr(),
      extent: core::mem::size_of_val(src),
    }
  }

  /// The offset origin — the address every offset is measured from. An **equality**.
  #[inline(always)]
  #[must_use]
  pub const fn addr(self) -> usize {
    self.origin
  }

  /// The byte extent of the referent. An **ordering** — see [`covers`](Self::covers).
  #[inline(always)]
  #[must_use]
  pub const fn extent(self) -> usize {
    self.extent
  }

  /// Whether a sink bound to `self` can materialize a parse read over `parsed`.
  ///
  /// Same origin, and an extent at least as long. The asymmetry is the point: a **longer**
  /// bound extent is the streaming capability, a **shorter** one lets structure be shaped by
  /// bytes the materialized tree will not contain. See the type docs.
  #[inline(always)]
  #[must_use]
  pub const fn covers(self, parsed: Self) -> bool {
    self.origin == parsed.origin && self.extent >= parsed.extent
  }
}

impl core::fmt::Display for SourceIdentity {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{:#x}+{}", self.origin, self.extent)
  }
}

/// The source trait for lexers.
pub trait Source<Cursor>: core::fmt::Debug {
  /// Whether `&Self` addresses the source bytes themselves, so that two unequal references
  /// **prove** two different sources.
  ///
  /// `false` — the default — means an inequality proves nothing and must never be used to
  /// refuse a parse. Exactly the `?Sized` backings can honestly answer `true`: for them the
  /// reference *is* the data. Every sized backing inherits `false`, including the reference
  /// forms (`&str`, `&[u8]`, `&BStr`) generated by `impl_source_for_borrowed!`, where the
  /// projection measures *where the pointer is stored* rather than where the bytes are.
  ///
  /// The one consumer is the source-identity check at the point an emitter is attached to an
  /// input (see [`Emitter::bound_source`](crate::Emitter::bound_source)). Declaring `true` for
  /// a sized backing turns `let b = a;` into a runtime panic; the default is the answer that
  /// cannot be wrong.
  ///
  /// It is a **defaulted** const on an unsealed trait: every existing implementation, in-tree
  /// and third-party, keeps compiling untouched.
  const REFERENT_IS_BYTES: bool = false;

  /// A type this `Source` can be sliced into.
  type Slice<'source>: Slice<'source>
  where
    Self: 'source;

  /// Returns `true` if the source is empty.
  fn is_empty(&self) -> bool;

  /// Length of the source
  fn len(&self) -> Cursor;

  /// Returns the entire source as a slice — the same contents as `slice(..)`.
  fn as_slice(&self) -> Self::Slice<'_>;

  /// Get a slice of the source at given range. This is analogous to
  /// `slice::get(range)`.
  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: RangeBounds<Cursor>;

  /// For `&str` sources, rounds `index` **down** to the closest `char` boundary at or below it,
  /// so the result is a position the source can be sliced at.
  ///
  /// "Closest" is *downward*, not nearest-in-either-direction: rounding up could move past a
  /// caller's intended cut. For binary sources (`&[u8]`) this returns `index` unchanged.
  ///
  /// An index **at or beyond** the end is returned unchanged in every implementation, so the
  /// result is a valid slice position only when the input was in range or exactly `len`. This
  /// is not a bounds check.
  #[inline]
  fn find_boundary(&self, index: Cursor) -> Cursor {
    index
  }

  /// Check if `index` is valid for this `Source`, that is:
  ///
  /// + It's not larger than the byte length of the `Source`.
  /// + (`str` only) It doesn't land in the middle of a UTF-8 code point.
  fn is_boundary(&self, index: Cursor) -> bool;
}

impl Source<usize> for [u8] {
  // Unsized: `&[u8]` addresses the bytes, so an unequal reference proves an unequal source.
  const REFERENT_IS_BYTES: bool = true;

  type Slice<'source>
    = &'source [u8]
  where
    Self: 'source;

  #[inline(always)]
  fn is_empty(&self) -> bool {
    <[u8]>::is_empty(self)
  }

  #[inline(always)]
  fn len(&self) -> usize {
    self.len()
  }

  #[inline(always)]
  fn as_slice(&self) -> Self::Slice<'_> {
    self
  }

  #[inline(always)]
  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: RangeBounds<usize>,
  {
    self.get((
      range.start_bound().map(|s| *s),
      range.end_bound().map(|s| *s),
    ))
  }

  #[inline(always)]
  fn is_boundary(&self, index: usize) -> bool {
    index <= self.len()
  }
}

impl Source<usize> for str {
  // Unsized: `&str` addresses the bytes, so an unequal reference proves an unequal source.
  const REFERENT_IS_BYTES: bool = true;

  type Slice<'source>
    = &'source str
  where
    Self: 'source;

  #[inline(always)]
  fn is_empty(&self) -> bool {
    <str>::is_empty(self)
  }

  #[inline(always)]
  fn len(&self) -> usize {
    <str>::len(self)
  }

  #[inline(always)]
  fn as_slice(&self) -> Self::Slice<'_> {
    self
  }

  #[inline(always)]
  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: RangeBounds<usize>,
  {
    self.get((
      range.start_bound().map(|s| *s),
      range.end_bound().map(|s| *s),
    ))
  }

  /// Rounds `index` **down** to the nearest UTF-8 code point boundary.
  ///
  /// For an index **inside** the source the result is a valid slice position.
  /// Indices at or beyond the end are returned unchanged, matching the byte
  /// sources — and such a result is a valid slice position only when it is exactly `len`. The
  /// previous wording claimed the result is "always a valid slice position" *and* that
  /// out-of-range indices come back unchanged, which cannot both be true: `len + 1` comes back
  /// unchanged and is not a slice position.
  ///
  /// Callers that may hold an out-of-range index check it (`is_boundary`, or a bound against
  /// `len`) rather than relying on this to clamp.
  #[inline(always)]
  fn find_boundary(&self, index: usize) -> usize {
    if index >= <str>::len(self) {
      return index;
    }
    let mut index = index;
    while !self.is_char_boundary(index) {
      index -= 1;
    }
    index
  }

  #[inline(always)]
  fn is_boundary(&self, index: usize) -> bool {
    self.is_char_boundary(index)
  }
}

macro_rules! impl_source_for_borrowed {
  ($target:ty) => {
    impl<'data> Source<usize> for &'data $target {
      // DELIBERATELY NOT `REFERENT_IS_BYTES = true`, however borrowed the backing looks.
      //
      // `&'data $target` is **Sized**: `&Self` here addresses the *pointer variable*, not the
      // bytes it names. Two `&str` locals holding the same pointer live at two addresses, and
      // so does a plain `let b = a;` copy — measured, at MSRV:
      //
      //     L::Source = str   : two borrows of one buffer agree: true
      //     L::Source = &str  : two equal &str locals agree:     false
      //     L::Source = &str  : a COPY of the same reference:    false
      //                         (their DATA addrs agree:         true)
      //
      // Setting it here would make the identity check refuse the crate's most ordinary
      // backing on code that is correct. "It's a borrowed source" is exactly the intuition
      // that produces the bug, which is why the reason is written down beside the macro that
      // would carry it.
      type Slice<'source>
        = <$target as Source<usize>>::Slice<'data>
      where
        Self: 'source;

      #[inline(always)]
      fn is_empty(&self) -> bool {
        <$target as Source<usize>>::is_empty(*self)
      }

      #[inline(always)]
      fn len(&self) -> usize {
        <$target as Source<usize>>::len(*self)
      }

      #[inline(always)]
      fn as_slice(&self) -> Self::Slice<'_> {
        <$target as Source<usize>>::as_slice(*self)
      }

      #[inline(always)]
      fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
      where
        R: RangeBounds<usize>,
      {
        <$target as Source<usize>>::slice(*self, range)
      }

      #[inline(always)]
      fn find_boundary(&self, index: usize) -> usize {
        <$target as Source<usize>>::find_boundary(*self, index)
      }

      #[inline(always)]
      fn is_boundary(&self, index: usize) -> bool {
        <$target as Source<usize>>::is_boundary(*self, index)
      }
    }
  };
}

impl_source_for_borrowed!([u8]);
impl_source_for_borrowed!(str);

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
