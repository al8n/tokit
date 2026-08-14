mod sealed {
  pub trait Sealed {}

  impl<T: ?Sized + Sealed> Sealed for &T {}
}

/// A trait for displaying values with configurable formatting options.
///
/// `DisplaySDL` (Structured Display Language) provides a flexible way to format values
/// with user-supplied options that control the output format. This is the base trait
/// that both [`DisplayCompact`] and [`DisplayPretty`] build upon.
///
/// # Design Philosophy
///
/// Unlike `Display` which has no configuration, `DisplaySDL` accepts an `Options` type
/// parameter that controls formatting behavior. This allows the same type to be formatted
/// in multiple ways (compact, pretty, custom) without needing multiple `Display` implementations.
///
/// # Implementation Note
///
/// **This trait is sealed and cannot be implemented directly.** Instead:
/// 1. Implement [`DisplayCompact`] and/or [`DisplayPretty`]
/// 2. Use [`CompactDisplay`] or [`PrettyDisplay`] wrappers
/// 3. Those wrappers automatically implement `DisplaySDL`
///
/// This design prevents direct implementation while providing the flexibility to
/// define custom formatting behaviors.
///
/// # Examples
///
/// ## Using DisplaySDL via CompactDisplay
///
/// ```rust
/// use tokora::utils::sdl_display::{DisplayCompact, DisplaySDL};
///
/// struct AST {
///     nodes: Vec<String>,
/// }
///
/// impl DisplayCompact for AST {
///     type Options = ();
///
///     fn fmt(&self, f: &mut core::fmt::Formatter<'_>, _options: &()) -> core::fmt::Result {
///         write!(f, "AST({})", self.nodes.join(","))
///     }
/// }
///
/// let ast = AST { nodes: vec!["a".into(), "b".into()] };
/// let compact = ast.display(&());
/// println!("{}", compact); // Uses DisplaySDL through CompactDisplay
/// ```
pub trait DisplaySDL: sealed::Sealed {
  /// The options type that controls formatting behavior.
  ///
  /// This can be any type that carries configuration for formatting.
  /// Common choices include:
  /// - `()`: No options needed
  /// - `usize`: For indentation level
  /// - Custom struct: For complex formatting options
  type Options: ?Sized;

  /// Formats the value with the given options.
  ///
  /// This is the core formatting method that receives both a formatter
  /// and user-supplied options.
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>, options: &Self::Options) -> core::fmt::Result;

  /// Returns a wrapper that implements `Display` with the given options.
  ///
  /// This allows you to use the formatted value with standard formatting macros.
  fn display<'a>(&'a self, options: &'a Self::Options) -> impl core::fmt::Display + 'a;
}

impl<T: DisplaySDL + ?Sized> DisplaySDL for &T {
  type Options = T::Options;

  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>, options: &Self::Options) -> core::fmt::Result {
    <T as DisplaySDL>::fmt(*self, f, options)
  }

  fn display<'a>(&'a self, options: &'a Self::Options) -> impl core::fmt::Display + 'a {
    <T as DisplaySDL>::display(*self, options)
  }
}

/// A trait for formatting values in a compact, space-efficient representation.
///
/// `DisplayCompact` is used when you want to minimize whitespace and produce
/// the most concise possible output. This is ideal for logging, inline display,
/// or when space is at a premium.
///
/// # Use Cases
///
/// - **Logging**: Compact output for log files where space matters
/// - **Inline display**: Single-line representations for debugging
/// - **Network protocols**: Minimized data transfer
/// - **Terminal output**: Fitting more information on screen
///
/// # Options Type
///
/// The associated `Options` type can be used to pass configuration to the
/// formatter. Common patterns include:
/// - `()`: No configuration needed
/// - `usize`: For recursion depth or other simple parameters
/// - Custom struct: For complex formatting options
///
/// # Examples
///
/// ## Simple Compact Display
///
/// ```rust
/// use tokora::utils::sdl_display::DisplayCompact;
///
/// struct Point { x: i32, y: i32 }
///
/// impl DisplayCompact for Point {
///     type Options = ();
///
///     fn fmt(&self, f: &mut core::fmt::Formatter<'_>, _: &()) -> core::fmt::Result {
///         write!(f, "({},{})", self.x, self.y)
///     }
/// }
///
/// let p = Point { x: 10, y: 20 };
/// println!("{}", p.display(&())); // (10,20)
/// ```
///
/// ## With Options
///
/// ```rust
/// use tokora::utils::sdl_display::DisplayCompact;
///
/// struct Tree {
///     value: i32,
///     children: Vec<Tree>,
/// }
///
/// impl DisplayCompact for Tree {
///     type Options = usize; // max depth
///
///     fn fmt(&self, f: &mut core::fmt::Formatter<'_>, depth: &usize) -> core::fmt::Result {
///         if *depth == 0 {
///             return write!(f, "...");
///         }
///         write!(f, "{}", self.value)?;
///         if !self.children.is_empty() {
///             write!(f, "(")?;
///             for (i, child) in self.children.iter().enumerate() {
///                 if i > 0 { write!(f, ",")?; }
///                 child.fmt(f, &(depth - 1))?;
///             }
///             write!(f, ")")?;
///         }
///         Ok(())
///     }
/// }
/// ```
pub trait DisplayCompact {
  /// The options type for controlling compact formatting.
  type Options: ?Sized;

  /// Formats the value in a compact, space-efficient way.
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>, options: &Self::Options) -> core::fmt::Result;

  /// Returns a wrapper that implements `Display` for compact formatting.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let compact = value.display(&options);
  /// println!("{}", compact);
  /// ```
  #[inline(always)]
  fn display<'a>(&'a self, options: &'a Self::Options) -> CompactDisplay<'a, Self> {
    CompactDisplay { t: self, options }
  }
}

impl<T: DisplayCompact + ?Sized> DisplayCompact for &T {
  type Options = T::Options;

  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>, options: &Self::Options) -> core::fmt::Result {
    (*self).fmt(f, options)
  }
}

/// A trait for formatting values in a human-friendly, readable representation.
///
/// `DisplayPretty` is used when you want to maximize readability with indentation,
/// whitespace, and multi-line output. This is ideal for debugging, documentation,
/// or when human comprehension is more important than space efficiency.
///
/// # Use Cases
///
/// - **Debugging**: Easily readable output for complex data structures
/// - **Pretty-printing ASTs**: Multi-line formatted abstract syntax trees
/// - **Configuration files**: Human-editable output formats
/// - **Error messages**: Clear, well-formatted diagnostic information
///
/// # Options Type
///
/// The associated `Options` type typically carries indentation information.
/// Common patterns include:
/// - `usize`: Current indentation level
/// - Custom struct: With indent size, style preferences, etc.
///
/// # Examples
///
/// ## Simple Pretty Display
///
/// ```rust
/// use tokora::utils::sdl_display::DisplayPretty;
///
/// struct Point { x: i32, y: i32 }
///
/// impl DisplayPretty for Point {
///     type Options = ();
///
///     fn fmt(&self, f: &mut core::fmt::Formatter<'_>, _: &()) -> core::fmt::Result {
///         writeln!(f, "Point {{")?;
///         writeln!(f, "  x: {},", self.x)?;
///         writeln!(f, "  y: {}", self.y)?;
///         write!(f, "}}")
///     }
/// }
///
/// let p = Point { x: 10, y: 20 };
/// println!("{}", p.display(&()));
/// // Output:
/// // Point {
/// //   x: 10,
/// //   y: 20
/// // }
/// ```
///
/// ## With Indentation
///
/// ```rust
/// use tokora::utils::sdl_display::DisplayPretty;
///
/// struct Tree {
///     value: i32,
///     children: Vec<Tree>,
/// }
///
/// impl DisplayPretty for Tree {
///     type Options = usize; // indentation level
///
///     fn fmt(&self, f: &mut core::fmt::Formatter<'_>, indent: &usize) -> core::fmt::Result {
///         let spaces = "  ".repeat(*indent);
///         writeln!(f, "{}Tree {{", spaces)?;
///         writeln!(f, "{}  value: {}", spaces, self.value)?;
///         if !self.children.is_empty() {
///             writeln!(f, "{}  children: [", spaces)?;
///             for child in &self.children {
///                 child.fmt(f, &(indent + 2))?;
///             }
///             writeln!(f, "{}  ]", spaces)?;
///         }
///         write!(f, "{}}}", spaces)
///     }
/// }
///
/// let tree = Tree {
///     value: 1,
///     children: vec![
///         Tree { value: 2, children: vec![] },
///         Tree { value: 3, children: vec![] },
///     ],
/// };
/// println!("{}", tree.display(&0));
/// ```
pub trait DisplayPretty {
  /// The options type for controlling pretty formatting.
  type Options: ?Sized;

  /// Formats the value in a human-friendly, readable way.
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>, options: &Self::Options) -> core::fmt::Result;

  /// Returns a wrapper that implements `Display` for pretty formatting.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let pretty = value.display(&0); // 0 indentation
  /// println!("{}", pretty);
  /// ```
  #[inline(always)]
  fn display<'a>(&'a self, options: &'a Self::Options) -> PrettyDisplay<'a, Self> {
    PrettyDisplay { t: self, options }
  }
}

impl<T: DisplayPretty + ?Sized> DisplayPretty for &T {
  type Options = T::Options;

  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>, options: &Self::Options) -> core::fmt::Result {
    (*self).fmt(f, options)
  }
}

/// A wrapper that implements `Display` for compact formatting.
///
/// This type is returned by [`DisplayCompact::display`] and bridges the gap
/// between the `DisplayCompact` trait (which requires options) and Rust's
/// standard `Display` trait (which does not).
///
/// # Examples
///
/// ```rust
/// use tokora::utils::sdl_display::DisplayCompact;
///
/// struct Data { value: i32 }
///
/// impl DisplayCompact for Data {
///     type Options = ();
///     fn fmt(&self, f: &mut core::fmt::Formatter<'_>, _: &()) -> core::fmt::Result {
///         write!(f, "Data({})", self.value)
///     }
/// }
///
/// let data = Data { value: 42 };
/// let display_wrapper = data.display(&());
///
/// // Now you can use it with format! and println!
/// assert_eq!(format!("{}", display_wrapper), "Data(42)");
/// ```
pub struct CompactDisplay<'a, T: ?Sized + DisplayCompact> {
  t: &'a T,
  options: &'a T::Options,
}

impl<T> core::fmt::Display for CompactDisplay<'_, T>
where
  T: DisplayCompact + ?Sized,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    self.t.fmt(f, self.options)
  }
}

impl<T> sealed::Sealed for CompactDisplay<'_, T> where T: DisplayCompact + ?Sized {}

impl<T> DisplaySDL for CompactDisplay<'_, T>
where
  T: DisplayCompact + ?Sized,
{
  type Options = T::Options;

  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>, options: &Self::Options) -> core::fmt::Result {
    self.t.fmt(f, options)
  }

  #[inline(always)]
  fn display<'a>(&'a self, options: &'a Self::Options) -> impl core::fmt::Display + 'a {
    self.t.display(options)
  }
}

/// A wrapper that implements `Display` for pretty formatting.
///
/// This type is returned by [`DisplayPretty::display`] and bridges the gap
/// between the `DisplayPretty` trait (which requires options) and Rust's
/// standard `Display` trait (which does not).
///
/// # Examples
///
/// ```rust
/// use tokora::utils::sdl_display::DisplayPretty;
///
/// struct Data { value: i32 }
///
/// impl DisplayPretty for Data {
///     type Options = usize; // indentation level
///     fn fmt(&self, f: &mut core::fmt::Formatter<'_>, indent: &usize) -> core::fmt::Result {
///         let spaces = "  ".repeat(*indent);
///         writeln!(f, "{}Data {{", spaces)?;
///         writeln!(f, "{}  value: {}", spaces, self.value)?;
///         write!(f, "{}}}", spaces)
///     }
/// }
///
/// let data = Data { value: 42 };
/// let display_wrapper = data.display(&0);
///
/// // Now you can use it with format! and println!
/// println!("{}", display_wrapper);
/// // Output:
/// // Data {
/// //   value: 42
/// // }
/// ```
pub struct PrettyDisplay<'a, T: ?Sized + DisplayPretty> {
  t: &'a T,
  options: &'a T::Options,
}

impl<T> core::fmt::Display for PrettyDisplay<'_, T>
where
  T: DisplayPretty + ?Sized,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    self.t.fmt(f, self.options)
  }
}

impl<T> sealed::Sealed for PrettyDisplay<'_, T> where T: DisplayPretty + ?Sized {}

impl<T> DisplaySDL for PrettyDisplay<'_, T>
where
  T: DisplayPretty + ?Sized,
{
  type Options = T::Options;

  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>, options: &Self::Options) -> core::fmt::Result {
    self.t.fmt(f, options)
  }

  #[inline(always)]
  fn display<'a>(&'a self, options: &'a Self::Options) -> impl core::fmt::Display + 'a {
    self.t.display(options)
  }
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
