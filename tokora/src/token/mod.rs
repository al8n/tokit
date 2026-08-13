pub use lit::*;
pub use punct::*;

#[cfg(feature = "pratt")]
#[cfg_attr(docsrs, doc(cfg(feature = "pratt")))]
pub use pratt::PrattToken;

mod lit;
mod punct;

#[cfg(feature = "pratt")]
mod pratt;

/// The core trait for token types used with Tokora.
///
/// `Token` defines the interface that all token types must implement to work with
/// Tokora's [`Lexer`](crate::Lexer) trait. It bridges the gap between lexical analysis and the
/// structured token representation needed for parsing.
///
/// # Design
///
/// The `Token` trait separates the Logos enum (the raw lexer output) from the structured
/// token type that's used in parsing. This separation allows you to:
///
/// - Add custom data or behavior to tokens beyond what Logos provides
/// - Normalize different Logos variants into a unified token type
/// - Implement additional traits and methods specific to your language
/// - Keep parsing logic separate from lexing logic
///
/// # Required Associated Types
///
/// - **`Kind`**: An enum representing token categories (e.g., `Identifier`, `Number`, `Plus`)
/// - **`Error`**: The error type produced by the lexer for invalid tokens
///
/// # Required Traits
///
/// Implementors must also implement:
/// - `Clone`: Tokens need to be cloneable for backtracking in parsers
/// - `Debug`: For debugging and error messages
///
/// ## Examples
///
/// ## Basic Implementation
///
/// ```rust,ignore
/// use tokora::Token;
/// use logos::Logos;
///
/// // The Logos enum (raw lexer output)
/// #[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
/// enum MyTokens {
///     #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
///     Identifier,
///
///     #[regex(r"[0-9]+")]
///     Number,
///
///     #[token("+")]
///     Plus,
/// }
///
/// // Token kinds (semantic categories)
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum TokenKind {
///     Identifier,
///     Number,
///     Plus,
/// }
///
/// // The structured token type
/// #[derive(Debug, Clone, PartialEq)]
/// struct MyToken {
///     kind: TokenKind,
///     // You can add extra fields here
///     // text: String,
///     // value: Option<i64>,
/// }
///
/// impl Token<'_> for MyToken {
///     type Char = char;
///     type Kind = TokenKind;
///     type Logos = MyTokens;
///
///     fn kind(&self) -> Self::Kind {
///         self.kind
///     }
/// }
///
/// impl From<MyTokens> for MyToken {
///     fn from(logos: MyTokens) -> Self {
///         let kind = match logos {
///             MyTokens::Identifier => TokenKind::Identifier,
///             MyTokens::Number => TokenKind::Number,
///             MyTokens::Plus => TokenKind::Plus,
///         };
///         MyToken { kind }
///     }
/// }
/// ```
///
/// ## Advanced: Storing Token Data
///
/// ```rust,ignore
/// // Token that stores the matched text
/// #[derive(Debug, Clone, PartialEq)]
/// struct RichToken<'a> {
///     kind: TokenKind,
///     text: &'a str,
/// }
///
/// impl<'a> Token<'a> for RichToken<'a> {
///     type Char = char;
///     type Kind = TokenKind;
///     type Logos = MyTokens;
///
///     fn kind(&self) -> Self::Kind {
///         self.kind
///     }
/// }
///
/// // Note: From<Logos> implementation would need access to the lexer
/// // to get the matched text, which typically happens in the Tokenizer
/// ```
///
/// ## Working with Bytes
///
/// ```rust,ignore
/// use tokora::Token;
/// use logos::Logos;
///
/// #[derive(Logos, Debug, Clone, Copy)]
/// #[logos(source = [u8])]
/// enum ByteTokens {
///     #[regex(br"[0-9]+")]
///     Number,
/// }
///
/// #[derive(Debug, Clone)]
/// struct ByteToken {
///     kind: ByteTokenKind,
/// }
///
/// impl Token<'_> for ByteToken {
///     type Char = u8;  // Using u8 for byte-based lexing
///     type Kind = ByteTokenKind;
///     type Logos = ByteTokens;
///
///     fn kind(&self) -> Self::Kind {
///         self.kind
///     }
/// }
/// ```
pub trait Token<'a>: Clone + core::fmt::Debug + 'a {
  /// The token kind discriminant used to categorize tokens.
  ///
  /// This is typically an enum that represents the semantic category of each token
  /// (e.g., `Identifier`, `Number`, `Operator`). It's separate from the Logos enum
  /// to allow for additional processing or normalization.
  ///
  /// # Requirements
  ///
  /// - Must be `Copy` for efficient passing
  /// - Must be `Debug` for error messages
  /// - Must be `PartialEq` and `Eq` for comparisons in parsers
  /// - Must be `Hash` for use in hash-based collections
  /// - Must be `'static`, because the kind doubles as the expected-set vocabulary of the
  ///   end-of-input error ([`UnexpectedEnd`](crate::error::UnexpectedEnd)'s `Set` is
  ///   `Clone + 'static` at the struct level) and as the element type of the dispatch
  ///   classification tables (`&'static [Kind]`). Every real kind is a fieldless
  ///   discriminant enum, so this costs nothing; a borrow-keyed kind such as
  ///   `Kind = &'inp str` is excluded, deliberately.
  type Kind: 'static
    + Copy
    + core::fmt::Debug
    + core::fmt::Display
    + PartialEq
    + Eq
    + core::hash::Hash;

  /// The error type of this token.
  type Error: Clone + core::fmt::Debug;

  /// Whether the lexer grammar this vocabulary belongs to **surfaces trivia as real
  /// tokens** — the totality half of the trivia concept ([`is_trivia`](Token::is_trivia)
  /// is the per-token identity half.)
  ///
  /// `true` is a **contract declaration**, not a checked fact: it promises that the lexer
  /// never silently discards source bytes — every byte of the input is covered by either
  /// an emitted token (trivia included) or a reported lexer error. A grammar with a
  /// lexer-level skip rule (e.g. logos `skip r"[ \t\r\n]+"`) must keep the default
  /// `false`.
  ///
  /// The lossless (`gap_kind`) `cst::Sink` refuses **at compile
  /// time** to be constructed over a lexer that does not declare this (see
  /// [`Lexer::SURFACES_TRIVIA`](crate::Lexer::SURFACES_TRIVIA), which defaults to this
  /// value): a skipped-trivia gap is indistinguishable at the event level from a
  /// dropped committed token, so materialization over a skipping lexer could never
  /// distinguish lost tokens from lost whitespace. Declaring `true` while skipping is a
  /// contract violation with the misused-checkpoint posture: no UB, no panic from this
  /// crate — materialization surfaces it as
  /// `cst::FinishError::UncoveredGap`.
  const SURFACES_TRIVIA: bool = false;

  /// The **read-frontier class** this vocabulary claims, for an adapter that cannot compute an
  /// exact frontier of its own — in practice the bundled logos adapter.
  ///
  /// `logos` exposes `span`, `slice` and `remainder`, but not its DFA's probe frontier, so
  /// `LogosLexer` cannot answer [`Lexer::read_frontier`](crate::Lexer::read_frontier) from
  /// anything it can see. It delegates to the vocabulary here — the same const-delegation shape
  /// [`SURFACES_TRIVIA`](Self::SURFACES_TRIVIA) uses, and for the same reason: the adapter is one
  /// blanket impl over every token type, so the `Token` impl is the only per-dialect site.
  ///
  /// The claim answers for an item whose scan recorded **no** frontier in the lexer state. An item
  /// whose scan did record one is answered by that value instead — see
  /// [`State::probed_to`](crate::State::probed_to), the per-item value channel a `logos` callback
  /// writes to.
  ///
  /// # The default is `Unbounded`, and claiming `SpanEnd` is a claim about the DFA
  ///
  /// [`SpanEnd`](crate::ReadFrontierClass::SpanEnd) promises that deciding an item never probes
  /// beyond that item's own span end. For a `logos` vocabulary that is **false whenever one
  /// pattern is a proper prefix of another** — `[0-9]+` beside `[0-9]+\.[0-9]+`, an integer beside
  /// a float or an exponent literal — because the engine probes into the longer pattern and then
  /// backtracks to the accepting prefix. It is a claim about the generated DFA, not only about
  /// callbacks, and the `conformance` kit's `run_partial` check is what falsifies a wrong one.
  ///
  /// The default is therefore the conservative
  /// [`Unbounded`](crate::ReadFrontierClass::Unbounded): a vocabulary that has not thought about
  /// it is not silently assumed safe. Under a non-final partial input that withholds every item
  /// until the stream is sealed, which is sound and costly — see
  /// [`Lexer::read_frontier`](crate::Lexer::read_frontier) for the cost.
  const READ_FRONTIER_CLASS: crate::ReadFrontierClass = crate::ReadFrontierClass::Unbounded;

  /// Returns the kind (category) of this token.
  ///
  /// This method is used extensively by parsers to determine what kind of token
  /// they're looking at without having to inspect the full token structure.
  ///
  /// ## Example
  ///
  /// ```rust,ignore
  /// let token = MyToken::from(logos_token);
  /// match token.kind() {
  ///     TokenKind::Identifier => handle_identifier(token),
  ///     TokenKind::Number => handle_number(token),
  ///     _ => handle_other(token),
  /// }
  /// ```
  fn kind(&self) -> Self::Kind;

  /// Returns `true` if this token represents trivia (whitespace, comments, etc.).
  ///
  /// Trivia tokens are lexical elements that don't affect the semantic meaning of code
  /// but are important for formatting, documentation, and code presentation.
  ///
  /// # Common Trivia Types
  ///
  /// - Whitespace: spaces, tabs, newlines, carriage returns
  /// - Comments: line comments, block comments, documentation comments
  /// - Language-specific formatting tokens
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::Token;
  /// use core::fmt;
  ///
  /// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  /// enum TokenKind {
  ///     Whitespace,
  ///     Comment,
  ///     Number,
  /// }
  ///
  /// impl fmt::Display for TokenKind {
  ///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
  ///         let name = match self {
  ///             Self::Whitespace => "whitespace",
  ///             Self::Comment => "comment",
  ///             Self::Number => "number",
  ///         };
  ///         f.write_str(name)
  ///     }
  /// }
  ///
  /// #[derive(Debug, Clone, PartialEq)]
  /// struct MyToken {
  ///     kind: TokenKind,
  /// }
  ///
  /// impl Token<'_> for MyToken {
  ///     type Kind = TokenKind;
  ///     type Error = ();
  ///
  ///     fn kind(&self) -> Self::Kind {
  ///         self.kind
  ///     }
  ///
  ///     fn is_trivia(&self) -> bool {
  ///         matches!(self.kind, TokenKind::Whitespace | TokenKind::Comment)
  ///     }
  /// }
  ///
  /// let token = MyToken { kind: TokenKind::Whitespace };
  /// assert!(token.is_trivia());
  /// ```
  fn is_trivia(&self) -> bool;
}

impl<'a, T: Token<'a>> Token<'a> for &'a T {
  type Kind = T::Kind;
  type Error = T::Error;

  const SURFACES_TRIVIA: bool = T::SURFACES_TRIVIA;
  const READ_FRONTIER_CLASS: crate::ReadFrontierClass = T::READ_FRONTIER_CLASS;

  #[inline(always)]
  fn kind(&self) -> Self::Kind {
    (*self).kind()
  }

  #[inline(always)]
  fn is_trivia(&self) -> bool {
    (*self).is_trivia()
  }
}

/// A trait for tokens that carry user-defined identifiers.
///
/// [`IdentifierToken`] focuses exclusively on identifier storage/matching. Keyword-related helpers live
/// in [`KeywordToken`], letting languages opt into only the capabilities they need.
///
/// ## Example
///
/// ```rust
/// use tokora::{Token, token::IdentifierToken};
/// use core::fmt;
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum MyTokenKind {
///     Identifier,
///     KeywordIf,
///     KeywordElse,
/// }
///
/// impl fmt::Display for MyTokenKind {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         let name = match self {
///             Self::Identifier => "identifier",
///             Self::KeywordIf => "if",
///             Self::KeywordElse => "else",
///         };
///         f.write_str(name)
///     }
/// }
///
/// #[derive(Debug, Clone, PartialEq)]
/// enum MyToken<'a> {
///     Identifier(&'a str),
///     If,
///     Else,
/// }
///
/// impl<'a> Token<'a> for MyToken<'a> {
///     type Kind = MyTokenKind;
///     type Error = ();
///
///     fn kind(&self) -> Self::Kind {
///         match self {
///            MyToken::Identifier(_) => MyTokenKind::Identifier,
///            MyToken::If => MyTokenKind::KeywordIf,
///            MyToken::Else => MyTokenKind::KeywordElse,
///         }
///     }
///
///     fn is_trivia(&self) -> bool {
///         false
///     }
/// }
///
/// impl<'a> IdentifierToken<'a> for MyToken<'a> {
///     fn is_identifier(&self) -> bool {
///         matches!(self, MyToken::Identifier(_))
///     }
/// }
///
/// let token = MyToken::Identifier("name");
/// assert!(token.is_identifier());
/// ```
pub trait IdentifierToken<'a>: Token<'a> {
  /// Returns `true` when the token is an identifier (user-defined name).
  fn is_identifier(&self) -> bool;
}

impl<'a, T: IdentifierToken<'a>> IdentifierToken<'a> for &'a T {
  #[inline(always)]
  fn is_identifier(&self) -> bool {
    T::is_identifier(self)
  }
}

/// A trait for tokens that represent keywords.
///
/// ## Example
///
/// ```rust
/// use tokora::{Token, token::KeywordToken};
/// use core::fmt;
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum MyTokenKind {
///     Identifier,
///     KeywordIf,
///     KeywordElse,
/// }
///
/// impl fmt::Display for MyTokenKind {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         let name = match self {
///             Self::Identifier => "identifier",
///             Self::KeywordIf => "if",
///             Self::KeywordElse => "else",
///         };
///         f.write_str(name)
///     }
/// }
///
/// #[derive(Debug, Clone, PartialEq)]
/// enum MyToken<'a> {
///     Identifier(&'a str),
///     If,
///     Else,
/// }
///
/// impl<'a> Token<'a> for MyToken<'a> {
///     type Kind = MyTokenKind;
///     type Error = ();
///
///     fn kind(&self) -> Self::Kind {
///         match self {
///            MyToken::Identifier(_) => MyTokenKind::Identifier,
///            MyToken::If => MyTokenKind::KeywordIf,
///            MyToken::Else => MyTokenKind::KeywordElse,
///         }
///     }
///
///     fn is_trivia(&self) -> bool {
///         false
///     }
/// }
///
/// impl<'a> KeywordToken<'a> for MyToken<'a> {
///     fn keyword(&self) -> Option<&'static str> {
///         match self.kind() {
///             MyTokenKind::KeywordIf => Some("if"),
///             MyTokenKind::KeywordElse => Some("else"),
///             _ => None,
///         }
///     }
/// }
///
/// let token = MyToken::If;
/// assert!(token.is_keyword());
/// assert_eq!(token.keyword(), Some("if"));
/// ```
pub trait KeywordToken<'a>: Token<'a> {
  /// Returns `true` when the token is any reserved keyword.
  #[inline(always)]
  fn is_keyword(&self) -> bool {
    self.keyword().is_some()
  }

  /// Returns the canonical spelling of the keyword, if this token represents one.
  fn keyword(&self) -> Option<&'static str>;
}

impl<'a, T: KeywordToken<'a>> KeywordToken<'a> for &'a T {
  #[inline(always)]
  fn is_keyword(&self) -> bool {
    T::is_keyword(self)
  }

  #[inline(always)]
  fn keyword(&self) -> Option<&'static str> {
    T::keyword(self)
  }
}

#[cfg(test)]
mod tests;
