use core::fmt;

use super::PositionedChar;

/// A trait for displaying values in a human-readable format.
///
/// `DisplayHuman` provides a standardized way to format values for human consumption,
/// with special handling for edge cases like non-UTF-8 bytes, positioned characters,
/// and various string types. The key difference from `Display` is that `DisplayHuman`
/// attempts to show the "most natural" representation for humans.
///
/// # Behavior
///
/// - **ASCII bytes (`u8`)**: Displayed as characters if ASCII, otherwise as numbers
/// - **Byte slices (`[u8]`)**: Displayed as UTF-8 strings if valid, otherwise as debug output
/// - **Character slices (`[char]`)**: Displayed as a string
/// - **Positioned characters**: Displays only the character, not the position
/// - **Standard types**: Delegates to normal `Display` implementation
///
/// # Use Cases
///
/// - **Error messages**: Show tokens/lexemes in readable form
/// - **Debug output**: Display parser state in understandable format
/// - **Logging**: Record what the parser saw in human-friendly way
/// - **Test assertions**: Compare expected vs actual in readable form
///
/// # Provided Implementations
///
/// tokora provides `DisplayHuman` for:
/// - All primitive types (`u8`, `char`, `str`, integers, floats)
/// - Byte slices (`[u8]`, `[u8; N]`)
/// - Character slices (`[char]`, `[char; N]`)
/// - `PositionedChar<T>` where `T: DisplayHuman`
/// - `Bytes` (with `bytes` feature)
/// - `BStr` (with `bstr` feature)
/// - `HipStr`/`HipByt` (with `hipstr` feature)
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::utils::human_display::DisplayHuman;
///
/// let ascii_byte: u8 = b'A';
/// let non_ascii_byte: u8 = 200;
///
/// assert_eq!(format!("{}", ascii_byte.display()), "A");
/// assert_eq!(format!("{}", non_ascii_byte.display()), "200");
/// ```
///
/// ## Byte Slice Handling
///
/// ```rust
/// use tokora::utils::human_display::DisplayHuman;
///
/// let utf8_bytes = b"hello";
/// let invalid_utf8 = &[0xFF, 0xFE];
///
/// // Valid UTF-8 displays as string
/// assert_eq!(format!("{}", utf8_bytes.display()), "hello");
///
/// // Invalid UTF-8 falls back to debug format
/// let debug_output = format!("{}", invalid_utf8.display());
/// ```
///
/// ## With Positioned Characters
///
/// ```rust
/// use tokora::utils::{PositionedChar, human_display::DisplayHuman};
///
/// let pc = PositionedChar::with_position('x', 42);
///
/// // Only shows the character, not the position
/// assert_eq!(format!("{}", pc.display()), "x");
/// ```
///
/// ## Custom Implementation
///
/// ```rust,ignore
/// use tokora::utils::human_display::DisplayHuman;
///
/// struct Token {
///     kind: TokenKind,
///     text: String,
/// }
///
/// impl DisplayHuman for Token {
///     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
///         write!(f, "{} '{}'", self.kind, self.text)
///     }
/// }
///
/// let token = Token { kind: TokenKind::Identifier, text: "foo".to_string() };
/// println!("{}", token.display()); // "Identifier 'foo'"
/// ```
pub trait DisplayHuman {
  /// Formats the value in a human-friendly way.
  ///
  /// This method should format the value in the most natural way for human readers,
  /// prioritizing readability over technical precision.
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result;

  /// Returns a wrapper that implements `Display` using the human-friendly format.
  ///
  /// This allows you to use `DisplayHuman` types with standard formatting macros
  /// like `format!`, `println!`, etc.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::utils::human_display::DisplayHuman;
  ///
  /// let bytes = b"hello";
  /// println!("{}", bytes.display()); // Prints: hello
  /// ```
  #[inline]
  fn display(&self) -> HumanDisplay<'_, Self> {
    HumanDisplay(self)
  }
}

impl<T: DisplayHuman + ?Sized> DisplayHuman for &T {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    (*self).fmt(f)
  }
}

impl DisplayHuman for () {
  #[inline]
  fn fmt(&self, _: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    Ok(())
  }
}

impl DisplayHuman for u8 {
  /// Formats ASCII bytes as characters, non-ASCII as numbers.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::human_display::DisplayHuman;
  ///
  /// let ascii = b'A';
  /// let non_ascii: u8 = 200;
  ///
  /// assert_eq!(format!("{}", ascii.display()), "A");
  /// assert_eq!(format!("{}", non_ascii.display()), "200");
  /// ```
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    if self.is_ascii() {
      write!(f, "{}", *self as char)
    } else {
      fmt::Display::fmt(self, f)
    }
  }
}

macro_rules! impl_display_human_for_primitive {
  ($($ty:ty),+) => {
    $(
      impl DisplayHuman for $ty {
        #[inline]
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
          fmt::Display::fmt(self, f)
        }
      }
    )*
  };
}

impl_display_human_for_primitive!(
  u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64, char, str
);

impl<T: DisplayHuman> DisplayHuman for PositionedChar<T> {
  /// Formats positioned characters showing only the character, not the position.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::{PositionedChar, human_display::DisplayHuman};
  ///
  /// let pc = PositionedChar::with_position('x', 100);
  /// assert_eq!(format!("{}", pc.display()), "x");
  /// ```
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    self.char_ref().fmt(f)
  }
}

impl DisplayHuman for [u8] {
  #[cfg(not(feature = "bstr_1"))]
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match core::str::from_utf8(self) {
      Ok(s) => s.fmt(f),
      Err(_) => core::fmt::Debug::fmt(self, f),
    }
  }

  #[cfg(feature = "bstr_1")]
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    bstr_1::BStr::new(self).fmt(f)
  }
}

impl DisplayHuman for [char] {
  /// Formats character slices as a string.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::human_display::DisplayHuman;
  ///
  /// let chars = ['h', 'e', 'l', 'l', 'o'];
  /// assert_eq!(format!("{}", chars.display()), "hello");
  /// ```
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    for c in self {
      c.fmt(f)?;
    }
    Ok(())
  }
}

impl<const N: usize> DisplayHuman for [u8; N] {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    self.as_slice().fmt(f)
  }
}

impl<const N: usize> DisplayHuman for [char; N] {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    self.as_slice().fmt(f)
  }
}

#[cfg(feature = "bytes_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bytes_1")))]
impl DisplayHuman for bytes_1::Bytes {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    self.as_ref().fmt(f)
  }
}

#[cfg(feature = "bstr_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bstr_1")))]
impl DisplayHuman for bstr_1::BStr {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    fmt::Display::fmt(self, f)
  }
}

#[cfg(feature = "hipstr_0_8")]
#[cfg_attr(docsrs, doc(cfg(feature = "hipstr_0_8")))]
const _: () = {
  use hipstr_0_8::{HipByt, HipStr};

  impl DisplayHuman for HipStr<'_> {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      fmt::Display::fmt(self, f)
    }
  }

  impl DisplayHuman for HipByt<'_> {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      DisplayHuman::fmt(self.as_ref(), f)
    }
  }
};

/// A wrapper that implements `Display` using [`DisplayHuman`] formatting.
///
/// This type is returned by [`DisplayHuman::display()`] and allows you to use
/// human-friendly formatting with standard Rust formatting macros.
///
/// # Examples
///
/// ```rust
/// use tokora::utils::human_display::DisplayHuman;
///
/// let bytes = b"Hello, world!";
/// let display_wrapper = bytes.display();
///
/// assert_eq!(format!("{}", display_wrapper), "Hello, world!");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HumanDisplay<'a, T: ?Sized>(&'a T);

impl<T: DisplayHuman + ?Sized> core::fmt::Display for HumanDisplay<'_, T> {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    self.0.fmt(f)
  }
}
