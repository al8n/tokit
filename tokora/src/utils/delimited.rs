use crate::span::{AsSpan, IntoSpan, SimpleSpan};

use super::IntoComponents;

/// A value delimited by opening and closing markers with source location tracking.
///
/// `Delimited<Open, Close, Data, S>` combines a data value with its opening and closing
/// delimiters, along with an optional span that tracks the source location of the entire
/// delimited construct. This is fundamental for parsing languages with paired delimiters
/// like parentheses, brackets, braces, quotes, or custom token pairs.
///
/// # Design
///
/// `Delimited` exposes its delimiters, span, and data through accessor methods. It
/// implements `Deref` and `DerefMut` to allow transparent access to the inner data
/// while keeping delimiter and span information available when needed.
///
/// # Common Patterns
///
/// ## Transparent Access via Deref
///
/// Thanks to `Deref`, you can call methods on the wrapped value directly:
///
/// ```rust
/// use tokora::{SimpleSpan, utils::Delimited};
///
/// let delimited = Delimited::new('(', ')', "hello", SimpleSpan::new(0, 7));
///
/// // Can call str methods directly
/// assert_eq!(delimited.len(), 5);
/// assert_eq!(delimited.to_uppercase(), "HELLO");
///
/// // But can still access delimiters and span
/// assert_eq!(delimited.open(), '(');
/// assert_eq!(delimited.close(), ')');
/// ```
///
/// ## Parsing Parenthesized Expressions
///
/// ```rust,ignore
/// use tokora::{SimpleSpan, utils::Delimited};
///
/// enum Expr {
///     Number(i64),
///     Paren(Box<Delimited<char, char, Expr>>),
/// }
///
/// // Track both the delimiters and the expression inside
/// let inner = Expr::Number(42);
/// let parens = Delimited::new('(', ')', inner, SimpleSpan::new(10, 14));
///
/// assert_eq!(parens.open(), '(');
/// assert_eq!(parens.close(), ')');
/// ```
///
/// ## String Literals with Quote Tracking
///
/// ```rust,ignore
/// use tokora::{SimpleSpan, utils::Delimited};
///
/// // Track which quote style was used
/// let single_quoted = Delimited::new('\'', '\'', "hello", SimpleSpan::new(0, 7));
/// let double_quoted = Delimited::new('"', '"', "world", SimpleSpan::new(10, 17));
///
/// // Later code can check quote style for semantic differences
/// match single_quoted.open() {
///     '\'' => println!("Single-quoted string"),
///     '"' => println!("Double-quoted string"),
///     _ => println!("Unknown quote type"),
/// }
/// ```
///
/// ## Generic Bracket Types
///
/// ```rust,ignore
/// use tokora::{SimpleSpan, utils::Delimited};
///
/// #[derive(Debug, Clone, Copy)]
/// enum BracketType {
///     Paren,   // ( )
///     Square,  // [ ]
///     Curly,   // { }
/// }
///
/// type BracketedExpr = Delimited<BracketType, BracketType, Expr>;
///
/// let list = Delimited::new(
///     BracketType::Square,
///     BracketType::Square,
///     vec![expr1, expr2, expr3],
///     SimpleSpan::new(0, 20),
/// );
/// ```
///
/// ## Mapping Values While Preserving Delimiters
///
/// ```rust
/// use tokora::{SimpleSpan, utils::Delimited};
///
/// let delimited_str = Delimited::new('"', '"', "42", SimpleSpan::new(0, 4));
///
/// // Parse the string, keeping the same delimiters and span
/// let parsed: Delimited<char, char, i32> = delimited_str.map_data(|s| s.parse().unwrap());
///
/// assert_eq!(*parsed, 42);
/// assert_eq!(parsed.open(), '"');
/// assert_eq!(parsed.close(), '"');
/// ```
///
/// ## Error Reporting for Mismatched Delimiters
///
/// ```rust,ignore
/// fn check_delimiters<O, C, D>(delimited: &Delimited<O, C, D>) -> Result<(), Error>
/// where
///     O: PartialEq<C>,
///     O: core::fmt::Debug,
///     C: core::fmt::Debug,
/// {
///     if !matches_pair(delimited.open(), delimited.close()) {
///         return Err(Error {
///             message: format!(
///                 "Mismatched delimiters: {:?} and {:?}",
///                 delimited.open(),
///                 delimited.close()
///             ),
///             span: delimited.span(),
///         });
///     }
///     Ok(())
/// }
/// ```
///
/// # Trait Implementations
///
/// - **`Deref` / `DerefMut`**: Access the inner data transparently
/// - **`Display`**: Delegates to the inner data's `Display` implementation
/// - **`AsSpan` / `IntoSpan`**: Extract just the span information
/// - **`IntoComponents`**: Destructure into `(S, Open, Close, Data)` tuple
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::{SimpleSpan, utils::Delimited};
///
/// let delimited = Delimited::new('(', ')', "content", SimpleSpan::new(0, 9));
///
/// assert_eq!(delimited.open(), '(');
/// assert_eq!(delimited.close(), ')');
/// assert_eq!(delimited.data(), &"content");
/// assert_eq!(*delimited, "content"); // Via Deref
/// ```
///
/// ## Destructuring
///
/// ```rust
/// use tokora::{SimpleSpan, utils::Delimited};
///
/// let delimited = Delimited::new('[', ']', 42, SimpleSpan::new(5, 8));
///
/// let (span, open, close, value) = delimited.into_components();
/// assert_eq!(span, SimpleSpan::new(5, 8));
/// assert_eq!(open, '[');
/// assert_eq!(close, ']');
/// assert_eq!(value, 42);
/// ```
///
/// ## Mutable Access
///
/// ```rust
/// use tokora::{SimpleSpan, utils::Delimited};
///
/// let mut delimited = Delimited::new('(', ')', 10, SimpleSpan::new(0, 3));
///
/// // Modify the data
/// *delimited += 5;
/// assert_eq!(*delimited, 15);
///
/// // Modify delimiters
/// *delimited.open_mut() = '[';
/// *delimited.close_mut() = ']';
/// assert_eq!(delimited.open(), '[');
/// assert_eq!(delimited.close(), ']');
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct Delimited<Open, Close, Data, S = SimpleSpan> {
  /// The opening delimiter.
  pub(super) open: Open,

  /// The closing delimiter.
  pub(super) close: Close,

  /// The source location span covering the entire delimited construct.
  ///
  /// This typically spans from the start of the opening delimiter
  /// to the end of the closing delimiter, including the content.
  pub(super) span: S,

  /// The wrapped data value.
  ///
  /// This is the actual content between the delimiters.
  pub(super) data: Data,
}

impl<Open, Close, Data, S> AsRef<S> for Delimited<Open, Close, Data, S> {
  #[inline(always)]
  fn as_ref(&self) -> &S {
    self.span_ref()
  }
}

impl<Open, Close, Data, S> AsSpan<S> for Delimited<Open, Close, Data, S> {
  #[inline(always)]
  fn as_span(&self) -> &S {
    AsRef::as_ref(self)
  }
}

impl<Open, Close, Data, S> IntoSpan<S> for Delimited<Open, Close, Data, S> {
  #[inline(always)]
  fn into_span(self) -> S {
    self.span
  }
}

impl<Open, Close, Data, S> core::ops::Deref for Delimited<Open, Close, Data, S> {
  type Target = Data;

  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.data
  }
}

impl<Open, Close, Data, S> core::ops::DerefMut for Delimited<Open, Close, Data, S> {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.data
  }
}

impl<Open, Close, Data, S> core::fmt::Display for Delimited<Open, Close, Data, S>
where
  Data: core::fmt::Display,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    self.data.fmt(f)
  }
}

impl<Open, Close, Data, S> core::error::Error for Delimited<Open, Close, Data, S>
where
  Data: core::error::Error,
  Open: core::fmt::Debug,
  Close: core::fmt::Debug,
  S: core::fmt::Debug,
{
}

impl<Open, Close, Data, S> IntoComponents for Delimited<Open, Close, Data, S> {
  type Components = (S, Open, Close, Data);

  #[inline(always)]
  fn into_components(self) -> Self::Components {
    (self.span, self.open, self.close, self.data)
  }
}

impl<Open, Close, Data, S> Delimited<&Open, &Close, &Data, &S> {
  /// Returns a copied version of the delimited value.
  #[inline(always)]
  pub const fn copied(&self) -> Delimited<Open, Close, Data, S>
  where
    Open: Copy,
    Close: Copy,
    Data: Copy,
    S: Copy,
  {
    Delimited {
      open: *self.open,
      close: *self.close,
      span: *self.span,
      data: *self.data,
    }
  }

  /// Returns a cloned version of the delimited value.
  #[inline(always)]
  pub fn cloned(&self) -> Delimited<Open, Close, Data, S>
  where
    Open: Clone,
    Close: Clone,
    Data: Clone,
    S: Clone,
  {
    self.map(Clone::clone, Clone::clone, Clone::clone, Clone::clone)
  }
}

impl<Open, Close, Data, S> Delimited<Open, Close, Data, S> {
  /// Create a new delimited value.
  #[inline(always)]
  pub const fn new(open: Open, close: Close, data: Data, span: S) -> Self {
    Self {
      open,
      close,
      span,
      data,
    }
  }

  /// Get a copy of the opening delimiter.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let delimited = Delimited::new('(', ')', "data", SimpleSpan::new(0, 6));
  /// assert_eq!(delimited.open(), '(');
  /// ```
  #[inline(always)]
  pub const fn open(&self) -> Open
  where
    Open: Copy,
  {
    self.open
  }

  /// Get a reference to the opening delimiter.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let delimited = Delimited::new(String::from("("), String::from(")"), "data", SimpleSpan::new(0, 6));
  /// assert_eq!(delimited.open_ref(), &"(");
  /// ```
  #[inline(always)]
  pub const fn open_ref(&self) -> &Open {
    &self.open
  }

  /// Get a mutable reference to the opening delimiter.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let mut delimited = Delimited::new('(', ')', "data", SimpleSpan::new(0, 6));
  /// *delimited.open_mut() = '[';
  /// assert_eq!(delimited.open(), '[');
  /// ```
  #[inline(always)]
  pub const fn open_mut(&mut self) -> &mut Open {
    &mut self.open
  }

  /// Get a copy of the closing delimiter.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let delimited = Delimited::new('(', ')', "data", SimpleSpan::new(0, 6));
  /// assert_eq!(delimited.close(), ')');
  /// ```
  #[inline(always)]
  pub const fn close(&self) -> Close
  where
    Close: Copy,
  {
    self.close
  }

  /// Get a reference to the closing delimiter.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let delimited = Delimited::new(String::from("("), String::from(")"), "data", SimpleSpan::new(0, 6));
  /// assert_eq!(delimited.close_ref(), &")");
  /// ```
  #[inline(always)]
  pub const fn close_ref(&self) -> &Close {
    &self.close
  }

  /// Get a mutable reference to the closing delimiter.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let mut delimited = Delimited::new('(', ')', "data", SimpleSpan::new(0, 6));
  /// *delimited.close_mut() = ']';
  /// assert_eq!(delimited.close(), ']');
  /// ```
  #[inline(always)]
  pub const fn close_mut(&mut self) -> &mut Close {
    &mut self.close
  }

  /// Get a copy of the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let delimited = Delimited::new('(', ')', "data", SimpleSpan::new(5, 10));
  /// assert_eq!(delimited.span(), SimpleSpan::new(5, 10));
  /// ```
  #[inline(always)]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Get a reference to the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let delimited = Delimited::new('(', ')', "data", SimpleSpan::new(5, 10));
  /// assert_eq!(delimited.span_ref(), &SimpleSpan::new(5, 10));
  /// ```
  #[inline(always)]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Get a mutable reference to the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let mut delimited = Delimited::new('(', ')', "data", SimpleSpan::new(5, 10));
  /// delimited.span_mut().set_end(15);
  /// assert_eq!(delimited.span().end(), 15);
  /// ```
  #[inline(always)]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Get a reference to the data.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let delimited = Delimited::new('(', ')', 42, SimpleSpan::new(0, 4));
  /// assert_eq!(*delimited.data(), 42);
  /// ```
  #[inline(always)]
  pub const fn data(&self) -> &Data {
    &self.data
  }

  /// Get a mutable reference to the data.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let mut delimited = Delimited::new('(', ')', 42, SimpleSpan::new(0, 4));
  /// *delimited.data_mut() = 100;
  /// assert_eq!(*delimited.data(), 100);
  /// ```
  #[inline(always)]
  pub const fn data_mut(&mut self) -> &mut Data {
    &mut self.data
  }

  /// Returns a reference to all components.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let delimited = Delimited::new('(', ')', String::from("hello"), SimpleSpan::new(0, 7));
  /// let borrowed: Delimited<&char, &char, &String, &SimpleSpan> = delimited.as_ref();
  /// assert_eq!(borrowed.data(), &"hello");
  /// ```
  #[inline(always)]
  pub const fn as_ref(&self) -> Delimited<&Open, &Close, &Data, &S> {
    Delimited {
      open: &self.open,
      close: &self.close,
      span: &self.span,
      data: &self.data,
    }
  }

  /// Returns a mutable reference to all components.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::Delimited};
  ///
  /// let mut delimited = Delimited::new('(', ')', String::from("hello"), SimpleSpan::new(0, 7));
  /// let mut borrowed: Delimited<&mut char, &mut char, &mut String, &mut SimpleSpan> = delimited.as_mut();
  /// borrowed.data_mut().push_str(" world");
  /// assert_eq!(delimited.data(), &"hello world");
  /// ```
  #[inline(always)]
  pub const fn as_mut(&mut self) -> Delimited<&mut Open, &mut Close, &mut Data, &mut S> {
    Delimited {
      open: &mut self.open,
      close: &mut self.close,
      span: &mut self.span,
      data: &mut self.data,
    }
  }

  /// Consume the delimited value and return the opening delimiter.
  #[inline(always)]
  pub fn into_open(self) -> Open {
    self.open
  }

  /// Consume the delimited value and return the closing delimiter.
  #[inline(always)]
  pub fn into_close(self) -> Close {
    self.close
  }

  /// Consume the delimited value and return the span.
  #[inline(always)]
  pub fn into_span(self) -> S {
    self.span
  }

  /// Consume the delimited value and return the data.
  #[inline(always)]
  pub fn into_data(self) -> Data {
    self.data
  }

  /// Decompose the delimited value into its components.
  #[inline(always)]
  pub fn into_components(self) -> (S, Open, Close, Data) {
    (self.span, self.open, self.close, self.data)
  }

  /// Map the data to a new value, preserving delimiters and span.
  #[inline]
  pub fn map_data<F, U>(self, f: F) -> Delimited<Open, Close, U, S>
  where
    F: FnOnce(Data) -> U,
  {
    Delimited {
      open: self.open,
      close: self.close,
      span: self.span,
      data: f(self.data),
    }
  }

  /// Map the opening delimiter to a new value, preserving the rest.
  #[inline]
  pub fn map_open<F, O>(self, f: F) -> Delimited<O, Close, Data, S>
  where
    F: FnOnce(Open) -> O,
  {
    Delimited {
      open: f(self.open),
      close: self.close,
      span: self.span,
      data: self.data,
    }
  }

  /// Map the closing delimiter to a new value, preserving the rest.
  #[inline]
  pub fn map_close<F, C>(self, f: F) -> Delimited<Open, C, Data, S>
  where
    F: FnOnce(Close) -> C,
  {
    Delimited {
      open: self.open,
      close: f(self.close),
      span: self.span,
      data: self.data,
    }
  }

  /// Map the span to a new value, preserving the rest.
  #[inline]
  pub fn map_span<F, T>(self, f: F) -> Delimited<Open, Close, Data, T>
  where
    F: FnOnce(S) -> T,
  {
    Delimited {
      open: self.open,
      close: self.close,
      span: f(self.span),
      data: self.data,
    }
  }

  /// Map all components to new values.
  #[inline]
  pub fn map<FO, FC, FD, FS, O, C, U, T>(
    self,
    fo: FO,
    fc: FC,
    fd: FD,
    fs: FS,
  ) -> Delimited<O, C, U, T>
  where
    FO: FnOnce(Open) -> O,
    FC: FnOnce(Close) -> C,
    FD: FnOnce(Data) -> U,
    FS: FnOnce(S) -> T,
  {
    Delimited {
      open: fo(self.open),
      close: fc(self.close),
      span: fs(self.span),
      data: fd(self.data),
    }
  }
}
